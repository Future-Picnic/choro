//! The review worker owns a fresh provider process, never the coding backend.
use super::*;
use anyhow::{ensure, Context as _, Result};
use ide_core::{code_review::*, AgentKind, AgentRuntimeKind};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

pub(crate) struct ReviewController {
    pub run: ReviewRun,
    pub active: bool,
    cancel: Arc<AtomicBool>,
}
impl Drop for ReviewController {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}
struct ReviewUpdate {
    run: ReviewRun,
    stopped: bool,
}

/// Capture unsaved context on the UI thread; load durable history in the worker.
struct ReviewContextSeed {
    timeline: Vec<AgentChatTimelineItem>,
    supplementary_guidance: String,
}

fn is_review_context(item: &AgentChatTimelineItem) -> bool {
    matches!(
        item,
        AgentChatTimelineItem::Message(
            AgentChatMessage::User { .. } | AgentChatMessage::Assistant { .. }
        ) | AgentChatTimelineItem::ProposedPlan(_)
    ) || matches!(item, AgentChatTimelineItem::WorkLog(entry) if entry.kind == WorkLogEntryKind::Command)
}

fn load_review_requirements(
    store: &LocalStore,
    parent: Uuid,
    doc: &str,
    seed: ReviewContextSeed,
    cancel: &AtomicBool,
) -> Result<ReviewRequirements> {
    // These queries are independent of the UI's 200-event page. Only context
    // kinds are loaded, so historical source projections need not be copied.
    let mut events = Vec::new();
    for kind in ["message", "proposed_plan", "work_log"] {
        ensure!(
            !cancel.load(Ordering::Acquire),
            "Review preparation cancelled"
        );
        events.extend(store.load_timeline_events_by_kind(parent, kind)?);
    }
    events.sort_by_key(|event| event.sequence);
    let mut items = Vec::<(AgentChatTimelineItem, u64)>::new();
    let mut indices = std::collections::HashMap::<String, usize>::new();
    let mut merge = |item: AgentChatTimelineItem, at: u64, replace: bool| -> Result<()> {
        ensure!(
            !cancel.load(Ordering::Acquire),
            "Review preparation cancelled"
        );
        if !is_review_context(&item) {
            return Ok(());
        }
        let (_, key, _, _) = persistence::stored_timeline_event_parts(&item)
            .context("Cannot identify saved review context")?;
        let key = key.context("Review context has no stable identity")?;
        if let Some(&index) = indices.get(&key) {
            // A paged plan may be older than a newer version already saved.
            let stale_plan = matches!((&items[index].0, &item),
                (AgentChatTimelineItem::ProposedPlan(saved), AgentChatTimelineItem::ProposedPlan(live))
                    if saved.revision > live.revision);
            if replace && !stale_plan {
                items[index].0 = item;
            }
        } else {
            indices.insert(key, items.len());
            items.push((item, at));
        }
        Ok(())
    };
    for event in events {
        let _: StoredTimelinePayload = serde_json::from_str(&event.payload_json)
            .with_context(|| format!("Cannot read saved review context event {}", event.id))?;
        if let Some(item) = timeline_item_from_store_event(&event) {
            merge(item, event.created_at, true)?;
        }
    }
    // Older conversations and interrupted writes may have messages in the
    // chat table without corresponding timeline rows. Supplement, never
    // replace, the more complete timeline representation of the same message.
    for message in store.load_chat_messages(parent)? {
        let item = match message.role.as_str() {
            "user" => AgentChatMessage::User {
                text: message.text,
                display_text: None,
                tags: vec![],
                created_at: message.created_at,
            },
            "assistant" => AgentChatMessage::Assistant {
                message_id: message.backend_message_id,
                text: message.text,
                created_at: message.created_at,
            },
            _ => continue,
        };
        merge(AgentChatTimelineItem::Message(item), message.created_at, false)?;
    }
    for item in seed.timeline {
        let (_, _, _, at) = persistence::stored_timeline_event_parts(&item)
            .context("Cannot capture current review context")?;
        merge(item, at, true)?;
    }
    ensure!(
        !cancel.load(Ordering::Acquire),
        "Review preparation cancelled"
    );
    // Retain order for events with equal timestamps while placing legacy or
    // unsaved messages in their chronological position.
    items.sort_by_key(|(_, at)| *at);
    let mut requirements = ReviewRequirements {
        user_requirements: vec![],
        decisions: vec![],
        checks: vec![],
        project_rules: vec![],
        supplementary_guidance: seed.supplementary_guidance,
    };
    if !doc.trim().is_empty() {
        requirements.user_requirements.push(doc.to_owned());
    }
    for (item, _) in items {
        match item {
            AgentChatTimelineItem::Message(AgentChatMessage::User { text, .. }) => {
                requirements.user_requirements.push(text);
            }
            AgentChatTimelineItem::Message(AgentChatMessage::Assistant { text, .. }) => {
                requirements.decisions.push(text);
            }
            AgentChatTimelineItem::ProposedPlan(plan) => {
                if !requirements.decisions.contains(&plan.markdown) {
                    requirements.decisions.push(plan.markdown);
                }
            }
            AgentChatTimelineItem::WorkLog(entry) => requirements.checks.push(ReviewCheck {
                description: serde_json::json!({"command":entry.title,
                    "provider_status":format!("{:?}",entry.status),"output":entry.detail}).to_string(),
                provenance: "Recorded coding-provider command event".into(),
                limitation: Some(
                    "Provider event status is not proof a check passed; reviewer did not rerun it and output may be absent".into(),
                ),
            }),
            _ => {}
        }
    }
    Ok(requirements)
}

impl CodeReview {
    pub fn from_run(run: ReviewRun) -> Self {
        let findings = run.findings.iter().map(|f| CodeReviewFinding {
            severity: match f.severity { ReviewSeverity::Critical => CodeReviewSeverity::Critical, ReviewSeverity::High => CodeReviewSeverity::High, ReviewSeverity::Medium => CodeReviewSeverity::Medium, ReviewSeverity::Low => CodeReviewSeverity::Low },
            location: Some(format!("{}:{}", f.location.path.display(), f.location.start)), title: f.title.clone(),
            detail: format!("Trigger: {}\n\nWhat happens: {}\n\nSuggested fix: {}\n\nReviewer challenge: {}", f.trigger, f.consequence, f.suggested_fix, f.challenge),
            impact: f.consequence.clone(), fix: Some(f.suggested_fix.clone()), selected: false, fix_requested: false,
        }).collect();
        let mut markdown = format!(
            "## Coverage\nCompletion: {}\nReviewed {} of {} files.\n{}\nFreshness: {:?}\n",
            if run.state == ReviewRunState::Complete && run.limitations.is_empty() {
                "complete"
            } else {
                "incomplete"
            },
            run.completed_files(),
            run.total_files(),
            run.limitations.join("\n"),
            run.freshness
        );
        for f in &run.findings {
            markdown.push_str(&format!("\n### {:?}: {}:{} — {}\n\nTrigger: {}\n\nWhat happens: {}\n\nSuggested fix: {}\n\nReviewer challenge: {}\n",f.severity,f.location.path.display(),f.location.start,f.title,f.trigger,f.consequence,f.suggested_fix,f.challenge));
        }
        Self {
            id: run.id.to_string(),
            markdown,
            findings,
            expanded: true,
            structured: Some(run),
        }
    }
}
fn update_card(session: &mut AgentChatSession, run: ReviewRun) -> CodeReview {
    let mut card = CodeReview::from_run(run);
    if let Some(AgentChatTimelineItem::CodeReview(old)) = session
        .timeline
        .iter()
        .find(|i| matches!(i, AgentChatTimelineItem::CodeReview(r) if r.id == card.id))
    {
        card.expanded = old.expanded;
        if let (Some(old_run), Some(new_run)) = (&old.structured, &card.structured) {
            for (i, f) in new_run.findings.iter().enumerate() {
                if let Some(j) = old_run.findings.iter().position(|o| o.id == f.id) {
                    if let Some(previous) = old.findings.get(j) {
                        card.findings[i].selected = previous.selected;
                        card.findings[i].fix_requested = previous.fix_requested;
                    }
                }
            }
        }
    }
    upsert_timeline_code_review(&mut session.timeline, card.clone());
    card
}
impl AgentChatState {
    /// Call once when constructing the desktop chat state, before exposing
    /// Review. This revokes unfinished runs even for unopened conversations.
    pub fn recover_reviews_on_startup(&mut self, cx: &mut Context<Self>) {
        static STARTED: AtomicBool = AtomicBool::new(false);
        if STARTED.swap(true, Ordering::AcqRel) {
            return;
        }
        self.review_startup_pending = true;
        let (tx, rx) = async_channel::bounded(1);
        std::thread::spawn(move || {
            let result =
                LocalStore::open_default().and_then(|s| s.interrupt_all_unfinished_reviews());
            let _ = tx.send_blocking(result);
        });
        cx.spawn(async move |this, cx| {
            if let Ok(result) = rx.recv().await {
                let _ = this.update(cx, |state, cx| match result {
                    Ok(runs) => {
                        state.review_startup_pending = false;
                        for run in runs {
                            let parent = run.parent_id;
                            if let Some(session) = state.sessions.get_mut(&parent) {
                                let card = update_card(session, run.clone());
                                persist_timeline_item(
                                    parent,
                                    AgentChatTimelineItem::CodeReview(card),
                                    cx,
                                );
                            }
                            state.review_controllers.insert(
                                parent,
                                ReviewController {
                                    run,
                                    active: false,
                                    cancel: Arc::new(AtomicBool::new(false)),
                                },
                            );
                            state.publish_change(
                                parent,
                                ChatChangeCategories {
                                    navigation: true,
                                    ..ChatChangeCategories::CONTENT
                                },
                                cx,
                            );
                        }
                    }
                    Err(error) => eprintln!("Review startup recovery failed: {error:#}"),
                });
            }
        })
        .detach();
    }
    pub fn review_run(&self, parent: Uuid) -> Option<&ReviewRun> {
        self.review_controllers.get(&parent).map(|c| &c.run)
    }
    pub fn review_blocks_writing(&self, parent: Uuid) -> bool {
        self.review_controllers
            .get(&parent)
            .is_some_and(|c| c.active)
    }
    /// Saved runs are still being revoked or revalidated; Review would be
    /// rejected by `start_review` until this clears.
    pub fn review_restoring(&self, parent: Uuid) -> bool {
        self.review_startup_pending || self.review_recovery.contains(&parent)
    }
    /// UI fixtures drive the controller's observable state without a worker.
    #[cfg(test)]
    pub(crate) fn set_review_fixture(
        &mut self,
        run: ReviewRun,
        active: bool,
        navigation: bool,
        cx: &mut Context<Self>,
    ) {
        let parent = run.parent_id;
        if let Some(session) = self.sessions.get_mut(&parent) {
            update_card(session, run.clone());
        }
        self.review_controllers.insert(
            parent,
            ReviewController {
                run,
                active,
                cancel: Arc::new(AtomicBool::new(false)),
            },
        );
        self.publish_change(
            parent,
            ChatChangeCategories {
                navigation,
                ..ChatChangeCategories::CONTENT
            },
            cx,
        );
    }
    pub fn cancel_review(&mut self, parent: Uuid, cx: &mut Context<Self>) {
        if let Some(controller) = self
            .review_controllers
            .get_mut(&parent)
            .filter(|c| c.active)
        {
            controller.cancel.store(true, Ordering::Release);
            if !controller.run.state.terminal() {
                controller.run.state = ReviewRunState::Cancelling;
            }
            self.publish_change(
                parent,
                ChatChangeCategories {
                    navigation: true,
                    ..ChatChangeCategories::CONTENT
                },
                cx,
            );
        }
    }
    pub fn start_review(
        &mut self,
        agent: AgentRecord,
        supplementary_guidance: String,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        ensure!(!self.review_startup_pending,"Saved review recovery is not complete; keep writing and retry Review after recovery finishes");
        ensure!(
            !self.review_blocks_writing(agent.id),
            "A review is already running in this conversation"
        );
        ensure!(
            !self.review_recovery.contains(&agent.id),
            "Restoring saved review results; try Review again in a moment"
        );
        ensure!(
            agent.runtime == AgentRuntimeKind::Chat
                && agent.delegation.is_none()
                && !agent.hidden_doc_assistant
                && agent.studio_context.is_none()
                && agent.design_context.is_none(),
            "Review is available in ordinary coding conversations"
        );
        ensure!(
            self.safe_for_delegation(agent.id),
            "Finish current work, queued messages and approvals before starting Review"
        );
        let session = self
            .sessions
            .get(&agent.id)
            .context("Conversation is unavailable")?;
        let mut seed = ReviewContextSeed {
            timeline: session
                .timeline
                .iter()
                .filter(|item| is_review_context(item))
                .cloned()
                .collect(),
            supplementary_guidance,
        };
        if let Some(plan) = &session.latest_plan {
            seed.timeline
                .push(AgentChatTimelineItem::ProposedPlan(plan.clone()));
        }
        let run = ReviewRun::new(
            agent.project_id.0,
            agent.id,
            format!("{:?}", agent.provider),
            agent.model_cli_value().unwrap_or(agent.model_label()).to_string(),
            format!("{:?}", agent.effort),
            review_now(),
        );
        let parent = agent.id;
        let run_id = run.id;
        let cancel = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = async_channel::unbounded();
        std::thread::Builder::new()
            .name("independent-code-review".into())
            .spawn({
                let run = run.clone();
                let cancel = cancel.clone();
                move || review_worker(agent, run, seed, cancel, sender)
            })?;
        self.review_controllers.insert(
            parent,
            ReviewController {
                run: run.clone(),
                active: true,
                cancel,
            },
        );
        if let Some(session) = self.sessions.get_mut(&parent) {
            let card = update_card(session, run);
            persist_timeline_item(parent, AgentChatTimelineItem::CodeReview(card), cx);
        }
        self.publish_change(
            parent,
            ChatChangeCategories {
                navigation: true,
                ..ChatChangeCategories::CONTENT
            },
            cx,
        );
        cx.spawn(async move |this, cx| {
            while let Ok(update) = receiver.recv().await {
                if this
                    .update(cx, |state, cx| {
                        state.apply_review_update(parent, run_id, update, cx)
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        Ok(())
    }
    fn apply_review_update(
        &mut self,
        parent: Uuid,
        id: Uuid,
        update: ReviewUpdate,
        cx: &mut Context<Self>,
    ) {
        let Some(controller) = self
            .review_controllers
            .get_mut(&parent)
            .filter(|c| c.run.id == id && c.active)
        else {
            return;
        };
        if update.run.id != id
            || update.run.parent_id != parent
            || update.run.project_id != controller.run.project_id
            || update.run.reviewer_id != controller.run.reviewer_id
            || update.run.snapshot_id != controller.run.snapshot_id
            || update.run.revision < controller.run.revision
        {
            return;
        }
        let navigation = update.stopped
            || controller.run.state != update.run.state
            || controller.run.freshness != update.run.freshness;
        controller.run = update.run.clone();
        controller.active = !update.stopped;
        if controller.cancel.load(Ordering::Acquire) && !controller.run.state.terminal() {
            controller.run.state = ReviewRunState::Cancelling;
        }
        if let Some(session) = self.sessions.get_mut(&parent) {
            let card = update_card(session, update.run);
            persist_timeline_item(parent, AgentChatTimelineItem::CodeReview(card), cx);
        }
        self.publish_change(
            parent,
            ChatChangeCategories {
                navigation,
                ..ChatChangeCategories::CONTENT
            },
            cx,
        );
    }
    pub(super) fn recover_reviews(&mut self, parent: Uuid, cx: &mut Context<Self>) {
        if self.review_blocks_writing(parent) {
            return;
        }
        self.review_recovery.insert(parent);
        let (tx, rx) = async_channel::bounded(1);
        std::thread::spawn(move || {
            let result = LocalStore::open_default().and_then(|store| {
                let mut runs = store.load_review_runs(parent)?;
                for run in &mut runs {
                    if !run.state.terminal() || run.freshness != ReviewFreshness::Current {
                        continue;
                    }
                    let input = store.load_review_input(run);
                    let revision = store.review_mutation_revision(parent);
                    *run =
                        store
                            .transact_review(run.id, None, |latest| {
                                if latest.state.terminal() {
                                    match (&input, &revision) {
                                        (Ok(input), Ok(revision)) => {
                                            revalidate_review(latest, input, *revision)
                                        }
                                        _ => latest.freshness = ReviewFreshness::Uncertain(
                                            "Saved snapshot or ownership evidence is unavailable"
                                                .into(),
                                        ),
                                    }
                                }
                                Ok(())
                            })?
                            .0;
                }
                Ok(runs)
            });
            let _ = tx.send_blocking(result);
        });
        cx.spawn(async move |this, cx| {
            let Ok(result) = rx.recv().await else {
                return;
            };
            let _ = this.update(cx, |state, cx| {
                state.review_recovery.remove(&parent);
                if state.review_blocks_writing(parent) {
                    return;
                }
                if result.as_ref().is_ok_and(|runs| runs.is_empty()) {
                    return;
                }
                if let Err(error) = &result {
                    eprintln!("Saved review recovery failed: {error:#}");
                    return;
                }
                if let Ok(runs) = result {
                    for mut run in runs {
                        if let Some(latest) = state
                            .review_run(parent)
                            .filter(|r| r.id == run.id && r.revision > run.revision)
                        {
                            run = latest.clone();
                        }
                        if let Some(session) = state.sessions.get_mut(&parent) {
                            let card = update_card(session, run.clone());
                            persist_timeline_item(
                                parent,
                                AgentChatTimelineItem::CodeReview(card),
                                cx,
                            );
                        }
                        state.review_controllers.insert(
                            parent,
                            ReviewController {
                                run,
                                active: false,
                                cancel: Arc::new(AtomicBool::new(false)),
                            },
                        );
                    }
                }
                state.publish_change(
                    parent,
                    ChatChangeCategories {
                        navigation: true,
                        ..ChatChangeCategories::CONTENT
                    },
                    cx,
                );
            });
        })
        .detach();
    }
    pub fn validate_review_for_fix(
        &mut self,
        parent: Uuid,
        review_id: String,
        cx: &mut Context<Self>,
    ) -> async_channel::Receiver<Result<bool>> {
        let (tx, rx) = async_channel::bounded(1);
        if self.review_blocks_writing(parent) {
            let _ = tx.try_send(Ok(false));
            return rx;
        }
        let structured = self.sessions.get(&parent).and_then(|s| {
            s.timeline.iter().find_map(|i| match i {
                AgentChatTimelineItem::CodeReview(r) if r.id == review_id => r.structured.clone(),
                _ => None,
            })
        });
        let Some(run) = structured else {
            let _ = tx.try_send(Ok(true));
            return rx;
        };
        let (update_tx, update_rx) = async_channel::bounded(1);
        std::thread::spawn(move || {
            let result = (|| -> Result<ReviewRun> {
                let store = LocalStore::open_default()?;
                let input = store.load_review_input(&run)?;
                let revision = store.review_mutation_revision(parent)?;
                Ok(store
                    .transact_review(run.id, None, |latest| {
                        revalidate_review(latest, &input, revision);
                        Ok(())
                    })?
                    .0)
            })();
            let _ = update_tx.send_blocking(result);
        });
        cx.spawn(async move |this, cx| {
            if let Ok(result) = update_rx.recv().await {
                let result = result.and_then(|run| {
                    let allowed = run.can_fix();
                    this.update(cx, |state, cx| {
                        if let Some(session) = state.sessions.get_mut(&parent) {
                            let card = update_card(session, run.clone());
                            persist_timeline_item(
                                parent,
                                AgentChatTimelineItem::CodeReview(card),
                                cx,
                            );
                        }
                        if let Some(c) = state
                            .review_controllers
                            .get_mut(&parent)
                            .filter(|c| c.run.id == run.id)
                        {
                            c.run = run;
                        }
                        state.publish_change(parent, ChatChangeCategories::CONTENT, cx);
                    })?;
                    Ok(allowed)
                });
                let _ = tx.send(result).await;
            }
        })
        .detach();
        rx
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context_store() -> (tempfile::TempDir, LocalStore, AgentRecord) {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
        let project = ide_core::Project::from_path(dir.path().join("project"));
        let agent = AgentRecord::new(
            project.id,
            project.path.clone(),
            "Review context",
            "Original task document",
            AgentKind::Codex,
            AgentModel::CodexDefault,
            AgentEffort::Medium,
            ide_core::agents::AgentAccessMode::FullAccess,
        );
        let mut config = ide_core::AppConfig::default();
        config.projects = vec![project];
        store.save_workspace_config(&config).unwrap();
        store.save_agents(std::slice::from_ref(&agent)).unwrap();
        (dir, store, agent)
    }

    fn save_context(store: &LocalStore, parent: Uuid, item: &AgentChatTimelineItem) {
        let (kind, key, payload, at) = persistence::stored_timeline_event_parts(item).unwrap();
        store.upsert_timeline_event(parent, kind, key, payload, at).unwrap();
    }

    fn user_context(text: &str, created_at: u64) -> AgentChatTimelineItem {
        AgentChatTimelineItem::Message(AgentChatMessage::User {
            text: text.into(),
            display_text: None,
            tags: vec![],
            created_at,
        })
    }

    #[test]
    fn review_includes_context_outside_the_loaded_page_and_unsaved_corrections() {
        let (_dir, store, agent) = context_store();
        for item in [
            user_context("Original requirement: retain drafts", 1),
            user_context("Correction: retain attachments too", 2),
            AgentChatTimelineItem::Message(AgentChatMessage::Assistant {
                message_id: Some("decision".into()),
                text: "Keep the existing authentication".into(),
                created_at: 3,
            }),
        ] {
            save_context(&store, agent.id, &item);
        }
        let plan = ProposedPlan::new("old-plan", "# Approved original plan");
        save_context(&store, agent.id, &AgentChatTimelineItem::ProposedPlan(plan.clone()));
        let mut check = WorkLogEntry::new(
            "old-check", "old-check", WorkLogEntryKind::Command,
            "cargo test", WorkLogStatus::Completed,
        ).detail(Some("Tests passed; integration tests were not run".into()));
        check.started_at = 4;
        check.updated_at = 4;
        save_context(&store, agent.id, &AgentChatTimelineItem::WorkLog(check));
        for i in 0..205 {
            save_context(&store, agent.id, &user_context(&format!("Later request {i}"), 10 + i));
        }
        let page = store.load_timeline_events_page(agent.id, None, 200).unwrap();
        let mut timeline = page.events.iter().filter_map(timeline_item_from_store_event).collect::<Vec<_>>();
        assert!(!timeline.iter().any(|item| matches!(item,
            AgentChatTimelineItem::Message(AgentChatMessage::User { text, .. })
                if text == "Original requirement: retain drafts")));
        // The chat table and timeline overlap; each message must appear once.
        store.upsert_chat_message(agent.id, "user", "Later request 204", 214, None).unwrap();
        timeline.push(user_context("Unsaved correction: support every provider", 215));
        timeline.push(AgentChatTimelineItem::ProposedPlan(plan));
        let requirements = load_review_requirements(
            &store, agent.id, &agent.doc,
            ReviewContextSeed { timeline, supplementary_guidance: "Supplementary rules".into() },
            &AtomicBool::new(false),
        ).unwrap();
        assert_eq!(&requirements.user_requirements[..3], &[
            "Original task document", "Original requirement: retain drafts", "Correction: retain attachments too",
        ]);
        assert_eq!(requirements.user_requirements.len(), 209);
        assert_eq!(requirements.user_requirements.last().unwrap(), "Unsaved correction: support every provider");
        assert_eq!(requirements.decisions, vec!["Keep the existing authentication", "# Approved original plan"]);
        assert_eq!(requirements.checks.len(), 1);
        assert!(requirements.checks[0].description.contains("integration tests were not run"));
        assert!(requirements.checks[0].limitation.as_ref().unwrap().contains("not proof"));
        assert_eq!(requirements.supplementary_guidance, "Supplementary rules");
    }

    #[test]
    fn review_merges_legacy_messages_and_live_updates_without_other_conversations() {
        let (_dir, store, agent) = context_store();
        let mut other = agent.clone();
        other.id = Uuid::new_v4();
        store.save_agents(&[agent.clone(), other.clone()]).unwrap();
        store.upsert_chat_message(agent.id, "user", "Legacy original request", 1, None).unwrap();
        store.upsert_chat_message(other.id, "user", "Another conversation's request", 1, None).unwrap();
        save_context(&store, other.id, &user_context("Another conversation's correction", 2));
        save_context(&store, agent.id, &user_context("Recent saved request", 2));
        let assistant = |text: &str| AgentChatTimelineItem::Message(AgentChatMessage::Assistant {
            message_id: Some("answer".into()), text: text.into(), created_at: 3,
        });
        store.upsert_chat_message(agent.id, "assistant", "Legacy partial answer", 3, Some("answer".into())).unwrap();
        save_context(&store, agent.id, &assistant("Saved answer"));
        let mut plan = ProposedPlan::new("plan", "# Newer saved plan");
        plan.revision = 2;
        save_context(&store, agent.id, &AgentChatTimelineItem::ProposedPlan(plan.clone()));
        plan.markdown = "# Stale paged plan".into();
        plan.revision = 1;
        let check = |status, detail: &str| AgentChatTimelineItem::WorkLog(
            WorkLogEntry::new("check", "check", WorkLogEntryKind::Command, "cargo test", status)
                .detail(Some(detail.into())),
        );
        save_context(&store, agent.id, &check(WorkLogStatus::InProgress, "Starting"));
        let requirements = load_review_requirements(
            &store, agent.id, "",
            ReviewContextSeed {
                timeline: vec![assistant("Unsaved final answer"), AgentChatTimelineItem::ProposedPlan(plan),
                    check(WorkLogStatus::Completed, "Finished successfully")],
                supplementary_guidance: String::new(),
            },
            &AtomicBool::new(false),
        ).unwrap();
        assert_eq!(requirements.user_requirements, vec!["Legacy original request", "Recent saved request"]);
        assert_eq!(requirements.decisions, vec!["Unsaved final answer", "# Newer saved plan"]);
        assert_eq!(requirements.checks.len(), 1);
        assert!(requirements.checks[0].description.contains("Finished successfully"));
        assert!(requirements.checks[0].description.contains("Completed"));
    }

    #[test]
    fn review_does_not_silently_drop_unreadable_history_or_ignore_cancellation() {
        let (_dir, store, agent) = context_store();
        store.upsert_timeline_event(agent.id, "message", Some("broken".into()), "{", 1).unwrap();
        let seed = || ReviewContextSeed { timeline: vec![], supplementary_guidance: String::new() };
        let error = load_review_requirements(&store, agent.id, "", seed(), &AtomicBool::new(false)).unwrap_err();
        assert!(error.to_string().contains("Cannot read saved review context event"));
        let error = load_review_requirements(&store, agent.id, "", seed(), &AtomicBool::new(true)).unwrap_err();
        assert!(error.to_string().contains("cancelled"));
    }

    fn run(parent: Uuid) -> ReviewRun {
        ReviewRun::new(
            Uuid::new_v4(),
            parent,
            "Claude".into(),
            "model".into(),
            "effort".into(),
            review_now(),
        )
    }
    #[test]
    fn active_review_reserves_only_its_parent_until_the_reviewer_stops() {
        let parent = Uuid::new_v4();
        let other = Uuid::new_v4();
        let mut state = AgentChatState::new();
        state.review_controllers.insert(
            parent,
            ReviewController {
                run: run(parent),
                active: true,
                cancel: Arc::new(AtomicBool::new(false)),
            },
        );
        assert!(state.review_blocks_writing(parent));
        assert!(!state.review_blocks_writing(other));
        assert!(!state.safe_for_delegation(parent));
        assert!(state.safe_for_delegation(other));
        assert!(state.handoff_must_wait(parent));
        assert!(!state.can_drain_queue(parent));
        // A terminal report is insufficient while the provider still lives.
        state.review_controllers.get_mut(&parent).unwrap().run.state = ReviewRunState::Complete;
        assert!(state.review_blocks_writing(parent));
        state.review_controllers.get_mut(&parent).unwrap().active = false;
        assert!(!state.review_blocks_writing(parent));
    }
    #[test]
    fn legacy_markdown_cards_deserialize_without_inventing_structured_evidence() {
        let value = serde_json::json!({"type":"code_review","id":"legacy","markdown":"Old review","expanded":false});
        let stored: StoredTimelinePayload = serde_json::from_value(value).unwrap();
        assert!(matches!(
            stored,
            StoredTimelinePayload::CodeReview {
                structured: None,
                ..
            }
        ));
        let mut reviewed = run(Uuid::new_v4());
        reviewed.state = ReviewRunState::Partial;
        let card = CodeReview::from_run(reviewed);
        let stored = StoredTimelinePayload::CodeReview {
            id: card.id,
            markdown: card.markdown,
            expanded: card.expanded,
            structured: card.structured,
        };
        let restored: StoredTimelinePayload =
            serde_json::from_value(serde_json::to_value(stored).unwrap()).unwrap();
        assert!(matches!(
            restored,
            StoredTimelinePayload::CodeReview {
                structured: Some(_),
                ..
            }
        ));
    }
    #[cfg(feature = "ui-layout-tests")]
    #[gpui::test]
    fn review_progress_preserves_navigation_and_late_events_cannot_revive_a_stopped_run(
        cx: &mut gpui::TestAppContext,
    ) {
        use gpui::AppContext;
        use std::{cell::RefCell, rc::Rc};
        let parent = Uuid::new_v4();
        let mut initial = run(parent);
        initial.state = ReviewRunState::Running;
        let id = initial.id;
        let entity = cx.new(|_| {
            let mut state = AgentChatState::new();
            state.review_controllers.insert(
                parent,
                ReviewController {
                    run: initial.clone(),
                    active: true,
                    cancel: Arc::new(AtomicBool::new(false)),
                },
            );
            state
        });
        let events = Rc::new(RefCell::new(Vec::new()));
        let observed = events.clone();
        let _observer = cx.new(|cx: &mut gpui::Context<()>| {
            cx.subscribe(&entity, move |_, _, event, cx| {
                if let AgentChatEvent::SessionChanged(change) = event {
                    observed.borrow_mut().push(change.categories);
                }
                let _ = cx;
            })
            .detach();
        });
        entity.update(cx, |state, cx| {
            for revision in 1..=100 {
                let mut progress = initial.clone();
                progress.revision = revision;
                state.apply_review_update(
                    parent,
                    id,
                    ReviewUpdate {
                        run: progress,
                        stopped: false,
                    },
                    cx,
                );
            }
            let accepted = state.review_run(parent).unwrap().clone();
            let mut foreign = accepted.clone();
            foreign.id = Uuid::new_v4();
            foreign.revision = 101;
            state.apply_review_update(
                parent,
                id,
                ReviewUpdate {
                    run: foreign,
                    stopped: true,
                },
                cx,
            );
            assert_eq!(state.review_run(parent).unwrap(), &accepted);
            assert!(state.review_blocks_writing(parent));
            state.apply_review_update(
                parent,
                id,
                ReviewUpdate {
                    run: initial.clone(),
                    stopped: false,
                },
                cx,
            );
            assert_eq!(state.review_run(parent).unwrap(), &accepted);
            let mut finished = accepted;
            finished.revision = 101;
            finished.state = ReviewRunState::Partial;
            state.apply_review_update(
                parent,
                id,
                ReviewUpdate {
                    run: finished.clone(),
                    stopped: true,
                },
                cx,
            );
            assert!(!state.review_blocks_writing(parent));
            let mut late = finished.clone();
            late.revision = 102;
            late.state = ReviewRunState::Running;
            state.apply_review_update(
                parent,
                id,
                ReviewUpdate {
                    run: late,
                    stopped: false,
                },
                cx,
            );
            assert_eq!(state.review_run(parent).unwrap(), &finished);
            assert!(!state.review_blocks_writing(parent));
        });
        cx.run_until_parked();
        let changes = events.borrow();
        assert_eq!(changes.len(), 101);
        assert!(changes[..100]
            .iter()
            .all(|c| c.conversation && !c.navigation));
        assert!(changes[100].navigation);
    }
    #[cfg(feature = "ui-layout-tests")]
    #[gpui::test]
    fn missing_conversation_and_duplicate_start_fail_before_reserving_or_exposing_source(
        cx: &mut gpui::TestAppContext,
    ) {
        use gpui::AppContext;
        let entity = cx.new(|_| AgentChatState::new());
        entity.update(cx, |state, cx| {
            for provider in [AgentKind::Codex, AgentKind::Claude, AgentKind::Gemini, AgentKind::OpenCode] {
                let model = AgentModel::default_for(provider);
                let mut agent = AgentRecord::new(
                    ide_core::ProjectId(Uuid::new_v4()),
                    "/unread/repository".into(),
                    "Coding",
                    "",
                    provider,
                    model,
                    model.default_effort(),
                    AgentAccessMode::FullAccess,
                );
                agent.runtime = AgentRuntimeKind::Chat;
                let id = agent.id;
                let error = state.start_review(agent, String::new(), cx).unwrap_err();
                assert!(error.to_string().contains("Conversation is unavailable"), "{provider:?}: {error:#}");
                assert!(state.review_run(id).is_none());
                assert!(state.sessions.is_empty());
            }
            let model = AgentModel::default_for(AgentKind::Claude);
            let agent = AgentRecord::new(
                ide_core::ProjectId(Uuid::new_v4()),
                "/unread/repository".into(),
                "Coding",
                "",
                AgentKind::Claude,
                model,
                model.default_effort(),
                AgentAccessMode::FullAccess,
            );
            let parent = agent.id;
            let current = run(parent);
            state.review_controllers.insert(
                parent,
                ReviewController {
                    run: current.clone(),
                    active: true,
                    cancel: Arc::new(AtomicBool::new(false)),
                },
            );
            assert!(state
                .start_review(agent, String::new(), cx)
                .unwrap_err()
                .to_string()
                .contains("already running"));
            assert_eq!(state.review_run(parent).unwrap(), &current);
        });
    }
    #[cfg(feature = "ui-layout-tests")]
    #[gpui::test]
    fn central_dispatch_rejects_local_queued_and_remote_style_submissions_before_appending(
        cx: &mut gpui::TestAppContext,
    ) {
        use gpui::AppContext;
        let parent = Uuid::new_v4();
        let entity = cx.new(|_| {
            let mut state = AgentChatState::new();
            state.review_controllers.insert(
                parent,
                ReviewController {
                    run: run(parent),
                    active: true,
                    cancel: Arc::new(AtomicBool::new(false)),
                },
            );
            state
        });
        entity.update(cx, |state, cx| {
            for queued in [false, true] {
                for read_only in [false, true] {
                    let result = crate::state::chat_dispatch::dispatch_loaded(
                        state,
                        parent,
                        "keep coding".into(),
                        None,
                        vec![],
                        AgentInteractionMode::Default,
                        queued,
                        read_only,
                        None,
                        cx,
                    );
                    assert!(result
                        .unwrap_err()
                        .to_string()
                        .contains("Code review is active"));
                }
            }
            assert!(state.sessions.is_empty());
        });
    }
}

fn review_worker(
    agent: AgentRecord,
    mut run: ReviewRun,
    seed: ReviewContextSeed,
    cancel: Arc<AtomicBool>,
    output: async_channel::Sender<ReviewUpdate>,
) {
    let mut backend: Option<ChatBackendController> = None;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<()> {
        let store = LocalStore::open_default()?;
        store.create_review_run(&run)?;
        ensure!(
            !store
                .load_delegations()?
                .iter()
                .any(|r| r.parent_agent_id == agent.id && !r.status.terminal()),
            "Finish or cancel the conversation's active Bandmate task before Review"
        );
        let root = agent.runtime_path().to_path_buf();
        let requirements = load_review_requirements(&store, agent.id, &agent.doc, seed, &cancel)?;
        let (evidence, limitations, revision) = store.review_ownership(agent.id)?;
        run.mutation_revision = revision;
        let storage = store.review_storage(run.id);
        let mut input = prepare_review(
            &mut run,
            &root,
            &storage,
            requirements,
            &evidence,
            &limitations,
            &cancel,
        )?;
        for (path, hash) in &input.source {
            if path.file_name().is_some_and(|n| n == "AGENTS.md")
                && run.files.iter().any(|f| {
                    f.path
                        .starts_with(path.parent().unwrap_or(std::path::Path::new("")))
                })
            {
                input.requirements.project_rules.push(format!(
                    "{}\n{}",
                    path.display(),
                    read_review_blob(&storage, hash)?
                ));
            }
        }
        store.save_review_input(&run, &input)?;
        ensure!(
            !cancel.load(Ordering::Acquire),
            "Review preparation cancelled"
        );
        let prepared = run.clone();
        run = store
            .transact_review(run.id, Some(0), |latest| {
                *latest = prepared;
                Ok(())
            })?
            .0;
        // Publish the real file list before MCP/provider startup can wait or
        // fail. Keeping the initial Preparing state until a model event hid
        // both coverage and the actual phase from the composer.
        output.send_blocking(ReviewUpdate { run: run.clone(), stopped: false })
            .map_err(|_| anyhow::anyhow!("Review view closed before provider startup"))?;
        let cwd = storage.join("runtime");
        std::fs::create_dir_all(&cwd)?;
        if agent.provider == AgentKind::OpenCode { protocol::review::prepare_open_code(&agent, &cwd)?; }
        let reviewer = fresh_reviewer_record(&agent, &run, cwd)?;
        let mut hosted_tools = if protocol::review::hosted(agent.provider) {
            Some(protocol::review::ReviewTools::start(&reviewer, cancel.clone(), run.deadline_at)?)
        } else { None };
        let prompt = hosted_tools.as_ref().map(|tools| tools.prompt()).unwrap_or_else(||
            "Review the frozen conversation scope using review_context. Follow the assigned batches, challenge candidates, account for every diff page, then finalize with review_report.".into());
        let (controller, events) =
            spawn_chat_backend_after_stop(reviewer, AgentInteractionMode::Default, None, None)?;
        backend = Some(controller);
        backend.as_ref().unwrap().send(ChatBackendCommand::SendTurn {text:prompt,mode:AgentInteractionMode::Default,read_only:true,turn_id:run.id.to_string()})?;
        let mut response = String::new();
        let mut last_message = None;
        let mut awaiting_response = false;
        let mut protocol_errors = 0usize;
        let mut published = u64::MAX;
        let mut last_freshness_check = std::time::Instant::now() - Duration::from_secs(2);
        loop {
            run = store.load_review_run(run.id)?;
            if cancel.load(Ordering::Acquire) && !run.state.terminal() {
                run = store
                    .transact_review(run.id, None, |r| {
                        if !r.state.terminal() {
                            r.state = ReviewRunState::Cancelling;
                        }
                        Ok(())
                    })?
                    .0;
                break;
            }
            if review_now() >= run.deadline_at && !run.state.terminal() {
                run = store
                    .transact_review(run.id, None, |r| {
                        r.stop(
                            ReviewRunState::Partial,
                            Some("Review deadline reached; coverage is incomplete".into()),
                            review_now(),
                        );
                        Ok(())
                    })?
                    .0;
                break;
            }
            if run.state.terminal() || last_freshness_check.elapsed() >= Duration::from_secs(2) {
                let mutation_revision = store.review_mutation_revision(agent.id)?;
                let mut checked = run.clone();
                revalidate_review(&mut checked, &input, mutation_revision);
                if checked.freshness != run.freshness {
                    run = store
                        .transact_review(run.id, None, |r| {
                            r.freshness = checked.freshness;
                            Ok(())
                        })?
                        .0;
                }
                last_freshness_check = std::time::Instant::now();
            }
            if run.revision != published {
                if output
                    .send_blocking(ReviewUpdate {
                        run: run.clone(),
                        stopped: false,
                    })
                    .is_err()
                {
                    cancel.store(true, Ordering::Release);
                    break;
                }
                published = run.revision;
            }
            if run.state.terminal() {
                break;
            }
            while let Ok(event) = events.try_recv() {
                match event {
                    ChatBackendEvent::Error(error) => anyhow::bail!("Reviewer failed: {error}"),
                    ChatBackendEvent::Status(AgentChatStatus::Running) => { awaiting_response = true; }
                    ChatBackendEvent::AssistantChunk { text, message_id } if hosted_tools.is_some() => {
                        // Codex can stream commentary before its final response.
                        // Parse only the final model message, never concatenate
                        // commentary or two separate JSON objects.
                        if message_id.is_some() && message_id != last_message { response.clear(); last_message = message_id; }
                        ensure!(response.len().saturating_add(text.len()) <= 1024 * 1024, "Reviewer response exceeds 1 MiB");
                        response.push_str(&text);
                    }
                    ChatBackendEvent::Status(AgentChatStatus::Idle) => {
                        if let Some(tools) = hosted_tools.as_mut() {
                            // Initial idle notifications are not model responses.
                            if response.trim().is_empty() {
                                ensure!(!awaiting_response, "Reviewer returned no review requests");
                                continue;
                            }
                            let next = match tools.respond(&response) {
                                Ok(next) => { protocol_errors = 0; next }
                                Err(error) => {
                                    ensure!(!cancel.load(Ordering::Acquire) && review_now() < run.deadline_at, "Review stopped while waiting for scoped tools");
                                    protocol_errors += 1;
                                    ensure!(protocol_errors <= 2, "Reviewer could not use the scoped review protocol: {error:#}");
                                    format!("Choro rejected that request: {error:#}. Return ONLY a JSON calls object using the four review tools and their inputSchema. Do not claim a clean result.")
                                }
                            };
                            response.clear();
                            last_message = None;
                            awaiting_response = false;
                            run = store.load_review_run(run.id)?;
                            if run.state.terminal() { break; }
                            ensure!(next.len() <= 8 * 1024 * 1024, "Review tool results exceed the response limit");
                            backend.as_ref().unwrap().send(ChatBackendCommand::SendTurn { text:next, mode:AgentInteractionMode::Default, read_only:true, turn_id:run.id.to_string() })?;
                            continue;
                        }
                        run = store
                            .transact_review(run.id, None, |r| {
                                r.stop(
                                    ReviewRunState::Partial,
                                    Some(
                                        "Reviewer ended without finalizing accounted coverage"
                                            .into(),
                                    ),
                                    review_now(),
                                );
                                Ok(())
                            })?
                            .0;
                        break;
                    }
                    ChatBackendEvent::PendingApproval(_)
                    | ChatBackendEvent::PendingUserInput(_)
                    => {
                        anyhow::bail!("Reviewer attempted an operation outside its read-only role")
                    }
                    ChatBackendEvent::ChangedFiles(changes) => {
                        // ACP emits an empty turn receipt even with all tools
                        // denied. Any actual filesystem activity remains fatal.
                        ensure!(changes.files.is_empty(), "Reviewer reported source mutations");
                    }
                    _ => {}
                }
            }
            if events.is_closed() && !run.state.terminal() {
                anyhow::bail!("Reviewer disconnected before completion");
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        Ok(())
    })).unwrap_or_else(|_| Err(anyhow::anyhow!("Review worker stopped unexpectedly; coverage is incomplete")));
    if let Some(controller) = backend.take() {
        let stopped = controller.stop_signal();
        controller.force_shutdown();
        while !stopped.is_stopped() {
            std::thread::sleep(Duration::from_millis(50));
        }
        drop(controller);
    }
    let state = if cancel.load(Ordering::Acquire) {
        ReviewRunState::Cancelled
    } else if review_now() >= run.deadline_at {
        ReviewRunState::Partial
    } else {
        ReviewRunState::Failed
    };
    let error = result.err().map(|e| format!("{e:#}"));
    if let Ok(store) = LocalStore::open_default() {
        let input = store.load_review_input(&run).ok();
        let revision = store.review_mutation_revision(agent.id).ok();
        if let Ok((latest, _)) = store.transact_review(run.id, None, |r| {
            if let (Some(input), Some(revision)) = (&input, revision) {
                revalidate_review(r, input, revision);
            } else if r.state == ReviewRunState::Complete {
                r.freshness = ReviewFreshness::Uncertain(
                    "Unable to revalidate the saved snapshot and mutation receipts".into(),
                );
            }
            r.stop(state, error.clone(), review_now());
            Ok(())
        }) {
            run = latest;
        } else {
            run.stop(state, error, review_now());
        }
    } else {
        run.stop(state, error, review_now());
    }
    let _ = output.send_blocking(ReviewUpdate { run, stopped: true });
}
