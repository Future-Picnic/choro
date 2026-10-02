//! Protocol 11's agent-only projection and actions. The Mac owns all state.
use super::*;
use crate::remote::protocol::{Action, AgentAction, RemoteQuery};
use crate::remote::{DevicePermission, RemoteError, RemoteResult};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

fn failure(e: impl std::fmt::Display) -> RemoteError {
    RemoteError::conflict(e.to_string())
}
fn id(value: &str) -> RemoteResult<Uuid> {
    Uuid::parse_str(value).map_err(|_| RemoteError::bad_request("Invalid identity"))
}
fn encode<T: serde::Serialize>(value: T) -> RemoteResult<Value> {
    serde_json::to_value(value).map_err(failure)
}
fn mode(value: &str) -> RemoteResult<AgentInteractionMode> {
    match value {
        "default" => Ok(AgentInteractionMode::Default),
        "plan" => Ok(AgentInteractionMode::Plan),
        _ => Err(RemoteError::bad_request("Choose Build or Plan")),
    }
}

impl CenterArea {
    pub(super) fn prepare_remote_submission(
        &mut self,
        agent: &AgentRecord,
        text: &str,
        recipients: &[Uuid],
        mode: AgentInteractionMode,
        cx: &mut Context<Self>,
    ) -> RemoteResult<bool> {
        if agent
            .delegation
            .as_ref()
            .is_some_and(|b| b.task_id.is_some())
        {
            if mode == AgentInteractionMode::Plan || !recipients.is_empty() {
                return Err(RemoteError::conflict(
                    "Planning and delegation belong to the lead",
                ));
            }
            return Ok(false);
        }
        if !self.set_expert_plan_mode(agent.id, mode, cx) {
            return Err(RemoteError::conflict("Could not update the team's mode"));
        }
        let mut stopped = false;
        if let Some(handle) = cx
            .try_global::<crate::state::delegation::DelegationHandle>()
            .cloned()
        {
            stopped = handle
                .0
                .update(cx, |coordinator, cx| {
                    coordinator.prepare_parent_message(agent.id, cx)
                })
                .map_err(failure)?;
        }
        let run = experts::authorize(
            agent.id,
            text,
            recipients,
            mode == AgentInteractionMode::Plan,
        )
        .map_err(failure)?;
        if self
            .resume_delegation_for_submission(run, text, !recipients.is_empty(), cx)
            .map_err(failure)?
        {
            stopped = false;
        }
        Ok(stopped)
    }

    fn phone_agent(&self, agent: &AgentRecord, cx: &App) -> Value {
        let mut value = serde_json::to_value(self.remote_agent_list_item(agent, cx)).unwrap();
        let runs = self.delegation_runs(agent.id, cx);
        use crate::state::agent_navigation::{provider_navigation_runtime, AgentNavigationRuntime};
        use crate::state::delegation::display::{run_activity, DelegationActivity};
        let provider_runtime = provider_navigation_runtime(
            agent, agent.project_id, self.agent_chats.read(cx), self.agent_activity.read(cx),
            self.terminals.read(cx), std::time::SystemTime::now(),
        );
        let child_needs_user = |id| {
            self.agent_chats.read(cx).session(id).is_some_and(|s| {
                s.pending_user_input.is_some() || s.pending_approval.is_some()
                    || s.status == AgentChatStatus::PlanReady
            })
        };
        let active = runs.iter().any(|r| run_activity(r, &child_needs_user) == DelegationActivity::Working);
        let band_attention = runs.iter().any(|r| run_activity(r, &child_needs_user) == DelegationActivity::Attention);
        value["lifecycle_status"] = json!(agent.status);
        value["pinned"] = json!(self.workspace.read(cx).is_agent_pinned(agent.id));
        value["team_active"] = json!(active);
        value["has_assignments"] = json!(!runs.is_empty());
        value["updated_at"] = json!(agent.updated_at);
        value["parent_agent_id"] = json!(agent
            .delegation
            .as_ref()
            .filter(|b| b.task_id.is_some())
            .map(|b| b.parent_agent_id));
        value["bandmate"] = json!(agent.expert_snapshot.as_ref().map(|e| &e.profile.name));
        value["attention_reason"] =
            json!(self.agent_chats.read(cx).session(agent.id).and_then(|s| {
                if s.pending_approval.is_some() {
                    Some("Approval needed")
                } else if s.pending_user_input.is_some() {
                    Some("Question")
                } else if s.status == AgentChatStatus::PlanReady {
                    Some("Plan ready")
                } else if s.status == AgentChatStatus::Failed {
                    Some("Failed")
                } else {
                    None
                }
            }));
        if value["attention_reason"].is_null() && provider_runtime == AgentNavigationRuntime::Waiting {
            value["attention_reason"] = json!("Waiting for you");
        }
        if value["attention_reason"].is_null() && band_attention {
            value["attention_reason"] = json!("Band needs attention");
        }
        if agent.status.is_finished() {
            value["attention_reason"] = Value::Null;
        }
        value["needs_attention"] = json!(!value["attention_reason"].is_null());
        value["navigation_status"] = json!(if agent.status.is_finished() {
            "idle"
        } else if !value["attention_reason"].is_null() {
            "waiting"
        } else if provider_runtime == AgentNavigationRuntime::Working || active {
            "working"
        } else {
            "idle"
        });
        value
    }

    pub(super) fn finish_remote_read(
        resource: String,
        query: RemoteQuery,
        device: String,
        agent: Option<AgentRecord>,
        live: Vec<AgentChatTimelineItem>,
        mut base: Value,
    ) -> RemoteResult<Value> {
        if resource == "attachment" {
            return crate::remote::attachments::preview(
                query.path.as_deref().unwrap_or_default(),
                &device,
            );
        }
        let agent = agent.ok_or_else(|| RemoteError::not_found("Agent not found"))?;
        let store = LocalStore::open_default().map_err(failure)?;
        if resource == "summary" {
            return encode(store.load_agent_summary(agent.id).map_err(failure)?);
        }
        let mut timeline = live;
        let mut initial_hidden_turn = false;
        if resource == "diff" {
            let stored = store.load_timeline_events(agent.id).map_err(failure)?;
            let mut history = stored
                .iter()
                .filter_map(timeline_item_from_store_event)
                .collect::<Vec<_>>();
            let live_ids = timeline
                .iter()
                .map(phone_item_id)
                .collect::<std::collections::HashSet<_>>();
            history.retain(|i| !live_ids.contains(&phone_item_id(i)));
            history.extend(timeline);
            timeline = history;
        } else {
            let before = query
                .cursor
                .as_deref()
                .map(|s| s.parse::<i64>().map(|n| n.saturating_add(1)))
                .transpose()
                .map_err(|_| RemoteError::bad_request("Invalid history cursor"))?;
            let page = store
                .load_timeline_events_page(agent.id, before, 160)
                .map_err(failure)?;
            base["history_cursor"] = json!(page.has_more.then(|| page
                .oldest_sequence
                .unwrap_or(0)
                .saturating_sub(1)
                .to_string()));
            if before.is_some() || timeline.is_empty() {
                // A page may begin in the middle of an internal maintenance turn.
                // Carry its visibility across the boundary instead of exposing its reply.
                let mut earlier = page.oldest_sequence;
                while let Some(sequence) = earlier {
                    let previous = store
                        .load_timeline_events_page(agent.id, Some(sequence), 160)
                        .map_err(failure)?;
                    if let Some(text) = previous.events.iter().rev().find_map(|event| {
                        match timeline_item_from_store_event(event) {
                            Some(AgentChatTimelineItem::Message(AgentChatMessage::User {
                                text,
                                ..
                            })) => Some(text),
                            _ => None,
                        }
                    }) {
                        initial_hidden_turn =
                            crate::state::agent_chat::search_turn_is_hidden(&text);
                        break;
                    }
                    earlier = if previous.has_more {
                        previous.oldest_sequence
                    } else {
                        None
                    };
                }
                timeline = page
                    .events
                    .iter()
                    .filter_map(timeline_item_from_store_event)
                    .collect();
                if timeline.is_empty() && before.is_none() {
                    timeline = Self::load_chat_session_hydration(&agent)
                        .map(|h| h.timeline)
                        .unwrap_or_default();
                }
            } else if timeline.len() > 160 {
                let start = timeline.len() - 160;
                initial_hidden_turn = timeline[..start]
                    .iter()
                    .rev()
                    .find_map(|item| match item {
                        AgentChatTimelineItem::Message(AgentChatMessage::User { text, .. }) => {
                            Some(crate::state::agent_chat::search_turn_is_hidden(text))
                        }
                        _ => None,
                    })
                    .unwrap_or(false);
                timeline = timeline.split_off(start);
            }
        }
        if resource == "diff" {
            return phone_diff(&agent, &timeline, &query);
        }
        let mut projected = phone_timeline(
            &timeline,
            agent.solo_rejoined_branch.is_some(),
            &device,
            initial_hidden_turn,
        );
        let before = query
            .cursor
            .as_deref()
            .and_then(|s| s.parse::<i64>().ok())
            .map(|n| n.saturating_add(1));
        let page = store
            .load_timeline_events_page(agent.id, before, 160)
            .map_err(failure)?;
        let mut identifiers =
            std::collections::HashMap::<String, std::collections::VecDeque<(Uuid, i64)>>::new();
        for event in page.events {
            if let Some(item) = timeline_item_from_store_event(&event) {
                identifiers
                    .entry(phone_item_id(&item))
                    .or_default()
                    .push_back((event.id, event.sequence));
            }
        }
        for item in &mut projected {
            if let Some((event_id, sequence)) = item["item_id"]
                .as_str()
                .and_then(|key| identifiers.get_mut(key))
                .and_then(|ids| ids.pop_front())
            {
                item["item_id"] = json!(event_id);
                item["sequence"] = json!(sequence);
            }
        }
        base["timeline"] = json!(projected);
        base["summary"] = encode(store.load_agent_summary(agent.id).map_err(failure)?)?;
        Ok(base)
    }

    pub(super) fn remote_extended_read(
        &mut self,
        resource: &str,
        query: RemoteQuery,
        permission: DevicePermission,
        device: &str,
        cx: &mut Context<Self>,
    ) -> RemoteResult<Value> {
        match resource {
            "capabilities" => {
                let mut value = crate::remote::protocol::capabilities(permission);
                value["delegation_enabled"] = json!(
                    ide_core::delegation::enabled()
                        && self.workspace.read(cx).beta_features.delegation
                );
                Ok(value)
            }
            "workspace" => {
                let workspace = self.workspace.read(cx);
                let descriptors = self.remote_projects(cx);
                let projects: Vec<Value> = workspace.projects.iter().filter_map(|p| {
                    let descriptor=descriptors.iter().find(|d|d.id==p.id.0.to_string())?;
                    let agents=self.agents.read(cx).records_for_project(p.id).into_iter().filter(|a|!a.hidden_doc_assistant && !a.status.is_finished())
                        .map(|a|self.phone_agent(&a,cx)).collect::<Vec<_>>();
                    Some(json!({"id":p.id,"name":p.name,"icon":p.icon,"icon_color":p.icon_color,"is_favorite":p.is_favorite,"section_id":p.section_id,"repositories":descriptor.repositories,"agents":agents}))
                }).collect();
                Ok(
                    json!({"projects":projects,"sections":workspace.project_sections.iter().map(|s|json!({"id":s.id,"name":s.name})).collect::<Vec<_>>(),"groups":ide_core::agent_navigation::project_groups(&workspace.projects,&workspace.project_sections),"pinned_agents":workspace.pinned_agents,"delegation_enabled":workspace.beta_features.delegation}),
                )
            }
            "bandmates" => Ok(
                json!({"profiles": experts::profiles().iter().map(|p|json!({"id":p.id,"revision":p.revision,"name":p.name,"description":p.description,"provider":p.provider,"model":p.model,"effort":p.effort})).collect::<Vec<_>>(), "delegation_enabled":self.workspace.read(cx).beta_features.delegation}),
            ),
            "attachment" => crate::remote::attachments::preview(
                query.path.as_deref().unwrap_or_default(),
                device,
            ),
            "search" => Self::remote_search_records(self.agents.read(cx).all_records(), query),
            "agent" | "summary" | "delegation" | "diff" => {
                let agent_id = id(query.agent_id.as_deref().unwrap_or_default())?;
                let agent = self
                    .agents
                    .read(cx)
                    .agent(agent_id)
                    .filter(|a| !a.hidden_doc_assistant)
                    .cloned()
                    .ok_or_else(|| RemoteError::not_found("Agent not found"))?;
                if resource == "delegation" {
                    return Ok(json!({"runs":self.phone_runs(agent_id,cx)}));
                }
                let mut result = encode(self.remote_agent_snapshot(agent_id, cx)?)?;
                result["agent"] = self.phone_agent(&agent, cx);

                let writable = permission != DevicePermission::ViewOnly
                    && (agent.access_mode != AgentAccessMode::FullAccess
                        || permission == DevicePermission::FullAccess);
                let child = agent
                    .delegation
                    .as_ref()
                    .is_some_and(|b| b.task_id.is_some());
                result["allowed_actions"] = json!({"send":writable,"configure":writable,"stop":permission!=DevicePermission::ViewOnly,"metadata":permission!=DevicePermission::ViewOnly,"plan":writable&&!child,"delegate":writable&&!child&&self.workspace.read(cx).beta_features.delegation,"approve":permission==DevicePermission::FullAccess,"ship":permission==DevicePermission::FullAccess,"reason":if writable {""}else{"Change this phone's permission in Desktop Settings → Remote access."}});
                result["runs"] = json!(self.phone_runs(agent_id, cx));
                result["queued"]=json!(self.agent_chats.read(cx).session(agent_id).map(|s|s.queued_turns.iter().map(|q|json!({"id":q.id,"text":q.display_text.as_ref().unwrap_or(&q.text),"created_at":q.created_at})).collect::<Vec<_>>()).unwrap_or_default());

                Ok(result)
            }
            _ => Err(RemoteError::not_found("Unknown agent resource")),
        }
    }

    fn phone_runs(&self, agent_id: Uuid, cx: &App) -> Vec<Value> {
        let parent = self
            .agents
            .read(cx)
            .agent(agent_id)
            .and_then(|a| a.delegation.as_ref())
            .map(|b| b.parent_agent_id)
            .unwrap_or(agent_id);
        self.delegation_runs(parent,cx).iter().map(|r|json!({
            "id":r.id,"parent_agent_id":r.parent_agent_id,"revision":r.revision,"status":r.status,"reason":r.pause_reason,
            "tasks":r.tasks.iter().map(|t|json!({"id":t.id,"title":t.plan.goal,"brief":t.plan,"bandmate":t.expert.profile.name,"status":t.status,"reason":t.reason,"revision":t.revision,"can_pause":t.status.occupies_slot(),"can_resume":matches!(t.status,ide_core::delegation::TaskStatus::Paused|ide_core::delegation::TaskStatus::Failed),"child_agent_id":t.attempt().map(|a|a.child_agent_id),"progress":t.attempt().map(|a|&a.progress),"reports":t.attempts.iter().filter_map(|a|a.result.as_ref().map(|report|json!({"revision":a.result_revision,"report":report}))).collect::<Vec<_>>() })).collect::<Vec<_>>()
        })).collect()
    }

    pub(super) fn remote_search_records(
        records: Vec<AgentRecord>,
        query: RemoteQuery,
    ) -> RemoteResult<Value> {
        let needle = crate::state::agent_chat::fold_search_text(
            query.query.as_deref().unwrap_or_default().trim(),
        );
        if needle.is_empty() {
            return Ok(json!({"results":[],"next_cursor":null}));
        }
        if needle.len() > 500 {
            return Err(RemoteError::bad_request("Search text is too long"));
        }
        let (cursor_revision, offset) = match query.cursor.as_deref() {
            None => (None, 0),
            Some(cursor) => {
                let (revision, offset) = cursor
                    .split_once(':')
                    .ok_or_else(|| RemoteError::bad_request("Invalid search cursor"))?;
                (
                    Some(revision),
                    offset
                        .parse::<usize>()
                        .map_err(|_| RemoteError::bad_request("Invalid search cursor"))?,
                )
            }
        };
        let selected = query.agent_id.as_deref().map(id).transpose()?;
        let store = LocalStore::open_default().map_err(failure)?;
        let mut results = Vec::new();
        for agent in records
            .iter()
            .filter(|a| !a.hidden_doc_assistant && selected.is_none_or(|id| id == a.id))
        {
            if selected.is_none()
                && crate::state::agent_chat::fold_search_text(&agent.title).contains(&needle)
            {
                results.push(json!({"agent_id":agent.id,"title":agent.title,"project_id":agent.project_id,"snippet":agent.title,"item_id":null}));
            }
            let events = store.load_timeline_events(agent.id).map_err(failure)?;
            let mut hidden_turn = false;
            for event in events {
                let Some(AgentChatTimelineItem::Message(message)) =
                    timeline_item_from_store_event(&event)
                else {
                    continue;
                };
                if let AgentChatMessage::User { text, .. } = &message {
                    hidden_turn = crate::state::agent_chat::search_turn_is_hidden(text);
                }
                if hidden_turn {
                    continue;
                }
                let Some(text) = crate::state::agent_chat::searchable_message_text(&message) else {
                    continue;
                };
                if crate::state::agent_chat::fold_search_text(&text).contains(&needle) {
                    results.push(json!({"agent_id":agent.id,"title":agent.title,"project_id":agent.project_id,"snippet":text.chars().take(240).collect::<String>(),"item_id":event.id,"sequence":event.sequence}));
                }
            }
        }
        let revision = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&results).unwrap())
        );
        if cursor_revision.is_some_and(|old| old != revision) {
            return Err(RemoteError::conflict(
                "Search results changed. Search again to refresh the results.",
            ));
        }
        let next = (offset.saturating_add(50) < results.len())
            .then(|| format!("{revision}:{}", offset + 50));
        Ok(
            json!({"results":results.into_iter().skip(offset).take(50).collect::<Vec<_>>(),"next_cursor":next}),
        )
    }

    pub(super) fn remote_extended_action(
        &mut self,
        request: AgentAction,
        permission: DevicePermission,
        cx: &mut Context<Self>,
    ) -> RemoteResult<Value> {
        if permission == DevicePermission::ViewOnly {
            return Err(RemoteError {
                status: 403,
                message: "This phone has View only permission".into(),
            });
        }
        let agent_id = id(&request.agent_id)?;
        let agent = self
            .agents
            .read(cx)
            .agent(agent_id)
            .filter(|a| !a.hidden_doc_assistant)
            .cloned()
            .ok_or_else(|| RemoteError::not_found("Agent not found"))?;
        if self.agent_chats.read(cx).session(agent_id).is_none() {
            let hydration = Self::load_chat_session_hydration(&agent);
            self.agent_chats.update(cx, |chats, cx| {
                let session = chats.ensure_session(agent_id, agent.title.clone(), cx);
                if let Some(h) = hydration {
                    Self::hydrate_chat_session_from_timeline(session, &agent, h.timeline);
                    session.proposed_plan = h.proposed_plan;
                }
                chats.publish_change(agent_id, crate::state::agent_chat::ChatChangeCategories::CONTENT, cx);
            });
        }
        match request.action {
            Action::Metadata {
                title,
                pinned,
                status,
            } => {
                if [title.is_some(), pinned.is_some(), status.is_some()]
                    .into_iter()
                    .filter(|v| *v)
                    .count()
                    != 1
                {
                    return Err(RemoteError::bad_request(
                        "Set one conversation property per command",
                    ));
                }
                if let Some(title) = title {
                    let title = title.trim();
                    if title.is_empty() || title.chars().count() > 200 {
                        return Err(RemoteError::bad_request("Use a title of 1–200 characters"));
                    }
                    self.agents
                        .update(cx, |a, cx| a.update_title(agent_id, title.into(), cx));
                    self.agent_chats
                        .update(cx, |s, cx| s.update_title(agent_id, title.into(), cx));
                }
                if let Some(pinned) = pinned {
                    self.workspace
                        .update(cx, |w, cx| w.set_agent_pinned(agent_id, pinned, cx));
                }
                if let Some(status) = status {
                    let status = match status.as_str() {
                        "done" => AgentStatus::Done,
                        "in_progress" => AgentStatus::InProgress,
                        _ => return Err(RemoteError::bad_request("Choose Done or Reopen")),
                    };
                    if status == AgentStatus::Done
                        && (self
                            .delegation_runs(agent_id, cx)
                            .iter()
                            .any(|r| !r.status.terminal())
                            || self
                                .agent_chats
                                .read(cx)
                                .session(agent_id)
                                .is_some_and(|s| {
                                    matches!(
                                        s.status,
                                        AgentChatStatus::Running | AgentChatStatus::Cancelling
                                    )
                                }))
                    {
                        return Err(RemoteError::conflict(
                            "Stop or finish the active work before marking this agent Done",
                        ));
                    }
                    self.agents
                        .update(cx, |a, cx| a.update_status(agent_id, status, cx));
                }
            }
            Action::Mode { mode: value } => {
                self.authorize_remote_agent_control(agent_id, permission, cx)?;
                let mode = mode(&value)?;
                if mode == AgentInteractionMode::Plan
                    && agent
                        .delegation
                        .as_ref()
                        .is_some_and(|b| b.task_id.is_some())
                {
                    return Err(RemoteError::conflict("Plan mode belongs to the lead"));
                }
                if !self.set_expert_plan_mode(agent_id, mode, cx) {
                    return Err(RemoteError::conflict("Could not update team mode"));
                }
                self.agent_chats.update(cx, |s, cx| {
                    if let Some(chat) = s.sessions.get_mut(&agent_id) {
                        chat.interaction_mode = mode;
                    }
                    s.publish_change(agent_id, crate::state::agent_chat::ChatChangeCategories::CONTROLS, cx);
                });
            }
            Action::Checklist {
                checklist_id,
                item_id,
                checked,
                expected_checked,
            } => {
                let current = self
                    .agent_chats
                    .read(cx)
                    .session(agent_id)
                    .and_then(|s| {
                        s.timeline.iter().find_map(|i| match i {
                            AgentChatTimelineItem::ReviewChecklist(c) if c.id == checklist_id => {
                                c.items.iter().find(|i| i.id == item_id).map(|i| i.checked)
                            }
                            _ => None,
                        })
                    })
                    .ok_or_else(|| RemoteError::not_found("Check no longer exists"))?;
                if current != expected_checked && current != checked {
                    return Err(RemoteError::conflict(
                        "This check changed on another device. Refresh and try again.",
                    ));
                }
                if current != checked {
                    self.agent_chats.update(cx, |s, cx| {
                        s.toggle_review_checklist_item(agent_id, &checklist_id, &item_id, cx)
                    });
                }
            }
            Action::ChecklistRetry { source_turn_id } => {
                self.authorize_remote_agent_control(agent_id, permission, cx)?;
                self.retry_agent_review_checklist(agent_id, source_turn_id, cx);
            }
            Action::ReviewFix {
                review_id,
                revision,
                finding_ids,
            } => {
                self.authorize_remote_agent_control(agent_id, permission, cx)?;
                let findings = self
                    .agent_chats
                    .read(cx)
                    .session(agent_id)
                    .and_then(|s| {
                        s.timeline.iter().find_map(|i| match i {
                            AgentChatTimelineItem::CodeReview(r) if r.id == review_id => {
                                Some(r.findings.clone())
                            }
                            _ => None,
                        })
                    })
                    .ok_or_else(|| RemoteError::not_found("Review not found"))?;
                let current_revision = self.agent_chats.read(cx).session(agent_id).and_then(|s| {
                    s.timeline.iter().find_map(|i| match i {
                        AgentChatTimelineItem::CodeReview(r) if r.id == review_id => Some(format!(
                            "{:x}",
                            Sha256::digest(format!("{}:{:?}", r.markdown, r.findings).as_bytes())
                        )),
                        _ => None,
                    })
                });
                if current_revision.as_deref() != Some(revision.as_str()) {
                    return Err(RemoteError::conflict(
                        "The review changed. Refresh it before requesting fixes.",
                    ));
                }
                if finding_ids.is_empty()
                    || finding_ids.iter().any(|k| {
                        !findings
                            .iter()
                            .enumerate()
                            .any(|(i, f)| *k == format!("{review_id}:{i}") && !f.fix_requested)
                    })
                {
                    return Err(RemoteError::conflict(
                        "Selected findings changed. Refresh the review.",
                    ));
                }
                self.agent_chats.update(cx, |s, cx| {
                    for (i, f) in findings.iter().enumerate() {
                        let selected = finding_ids.contains(&format!("{review_id}:{i}"));
                        if f.selected != selected {
                            s.toggle_code_review_finding_selected(agent_id, &review_id, i, cx);
                        }
                    }
                });
                self.request_agent_code_review_fix(agent_id, review_id, true, cx);
            }
            Action::Summary => {
                self.authorize_remote_agent_control(agent_id, permission, cx)?;
                if !self.request_agent_summary(agent_id, cx) {
                    return Err(RemoteError::conflict(
                        "A summary cannot be refreshed right now",
                    ));
                }
            }
            Action::Delegation {
                run_id,
                task_id,
                revision,
                operation,
            } => {
                self.authorize_remote_agent_control(agent_id, permission, cx)?;
                let run_id = id(&run_id)?;
                let task_id = task_id.as_deref().map(id).transpose()?;
                let handle = cx
                    .try_global::<crate::state::delegation::DelegationHandle>()
                    .cloned()
                    .ok_or_else(|| RemoteError::conflict("Band is unavailable"))?;
                let run = handle
                    .0
                    .read(cx)
                    .runs
                    .iter()
                    .find(|r| r.id == run_id && r.parent_agent_id == agent_id)
                    .cloned()
                    .ok_or_else(|| RemoteError::not_found("Team not found for this lead"))?;
                if run.revision != revision {
                    return Err(RemoteError::conflict(
                        "The team changed. Refresh before taking this action.",
                    ));
                }
                handle
                    .0
                    .update(cx, |c, cx| match (operation.as_str(), task_id) {
                        ("pause", Some(t)) => c.pause_task(run_id, t, cx),
                        ("resume", Some(t)) => c.resume_task(run_id, t, cx),
                        ("pause", None) => c.pause(run_id, cx),
                        ("resume", None) => c.resume(run_id, cx),
                        ("end", None) if run.status.stopped() => c.end_run(run_id, cx),
                        _ => Err(anyhow::anyhow!("This team action is not available")),
                    })
                    .map_err(failure)?;
            }
        }
        cx.notify();
        Ok(json!({"accepted":true}))
    }
}

fn phone_item_id(item: &AgentChatTimelineItem) -> String {
    match item {
        AgentChatTimelineItem::Message(AgentChatMessage::User {
            created_at, text, ..
        }) => format!("user-{created_at}-{:x}", Sha256::digest(text.as_bytes())),
        AgentChatTimelineItem::Message(AgentChatMessage::Assistant {
            message_id,
            created_at,
            ..
        }) => format!(
            "assistant-{}",
            message_id.clone().unwrap_or_else(|| created_at.to_string())
        ),
        AgentChatTimelineItem::Message(AgentChatMessage::Thought {
            message_id,
            created_at,
            ..
        }) => format!(
            "thought-{}",
            message_id.clone().unwrap_or_else(|| created_at.to_string())
        ),
        AgentChatTimelineItem::ReviewChecklist(c) => c.id.clone(),
        AgentChatTimelineItem::CodeReview(r) => r.id.clone(),
        AgentChatTimelineItem::ChangedFiles(c) => {
            format!("changes-{}", c.turn_id.clone().unwrap_or_default())
        }
        _ => format!("item-{:x}", Sha256::digest(format!("{item:?}").as_bytes())),
    }
}
fn phone_timeline(
    timeline: &[AgentChatTimelineItem],
    rejoined: bool,
    device: &str,
    mut hidden_turn: bool,
) -> Vec<Value> {
    timeline.iter().filter_map(|item| {
        if let AgentChatTimelineItem::Message(AgentChatMessage::User{text,..})=item {hidden_turn=crate::state::agent_chat::search_turn_is_hidden(text);}
        if hidden_turn && matches!(item,AgentChatTimelineItem::Message(_)|AgentChatTimelineItem::WorkLog(_)) {return None;}

        let mut value=match item {
            AgentChatTimelineItem::ReviewChecklist(c)=>json!({"type":"review_checklist","id":c.id,"source_turn_id":c.source_turn_id,"status":format!("{:?}",c.status).to_lowercase(),"items":c.items.iter().map(|i|json!({"id":i.id,"flow":i.flow,"action":i.action,"expected":i.expected,"checked":i.checked})).collect::<Vec<_>>() }),
            AgentChatTimelineItem::CodeReview(r)=>json!({"type":"code_review","id":r.id,"revision":format!("{:x}",Sha256::digest(format!("{}:{:?}",r.markdown,r.findings).as_bytes())),"markdown":r.markdown,"coverage":r.coverage(),"coverage_complete":r.coverage_complete(),"findings":r.findings.iter().enumerate().map(|(i,f)|json!({"id":format!("{}:{i}",r.id),"severity":f.severity.label(),"title":f.title,"location":f.location,"detail":f.detail,"impact":f.impact,"fix":f.fix,"fix_requested":f.fix_requested})).collect::<Vec<_>>() }),
            AgentChatTimelineItem::DelegationGroup{run_id,..}=>json!({"type":"delegation","run_id":run_id}),
            _=>serde_json::to_value(super::remote_bridge::timeline_item_dto(item, super::remote_bridge::fixable_verification_id(timeline).as_deref(),rejoined)?).ok()?,
        };
        if let AgentChatTimelineItem::Message(AgentChatMessage::User{text,display_text,tags,..})=item {
            let (literal,paths)=split_prompt_attached_files(text);
            value["text"]=json!(display_text.as_deref().unwrap_or(&literal));value["tags"]=json!(tags);
            value["attachment_count"]=json!(paths.len());
            value["attachments"]=json!(crate::remote::attachments::descriptors(&paths,device));
        }
        if let AgentChatTimelineItem::ChangedFiles(c)=item {value["turn_id"]=json!(c.turn_id);}
        value["item_id"]=json!(phone_item_id(item));Some(value)
    }).collect()
}
fn phone_diff(
    agent: &AgentRecord,
    timeline: &[AgentChatTimelineItem],
    query: &RemoteQuery,
) -> RemoteResult<Value> {
    let turn = query
        .turn_id
        .as_deref()
        .ok_or_else(|| RemoteError::bad_request("Select the changed-file receipt to review"))?;
    let path = query.path.as_deref().unwrap_or_default();
    let receipt = timeline
        .iter()
        .rev()
        .find_map(|i| match i {
            AgentChatTimelineItem::ChangedFiles(c) if c.turn_id.as_deref() == Some(turn) => Some(c),
            _ => None,
        })
        .ok_or_else(|| RemoteError::not_found("This change receipt is unavailable"))?;
    let normalized = super::remote_bridge::normalize_remote_diff_path(
        agent.runtime_path(),
        std::path::Path::new(path),
    );
    if !receipt.conversation_files().any(|f| {
        super::remote_bridge::normalize_remote_diff_path(agent.runtime_path(), &f.path)
            == normalized
    }) {
        return Err(RemoteError::not_found("File is not part of this receipt"));
    }
    let snapshot = receipt
        .snapshot_id
        .ok_or_else(|| RemoteError::not_found("This receipt has no saved diff"))?;
    let saved = LocalStore::open_default()
        .and_then(|s| s.load_agent_diff_snapshot(snapshot))
        .map_err(failure)?
        .ok_or_else(|| RemoteError::not_found("Saved diff is unavailable"))?;
    let file = saved
        .files
        .iter()
        .find(|f| {
            super::remote_bridge::normalize_remote_diff_path(agent.runtime_path(), &f.path)
                == normalized
        })
        .ok_or_else(|| RemoteError::not_found("File is absent from saved diff"))?;
    let offset = query
        .cursor
        .as_deref()
        .unwrap_or("0")
        .parse::<usize>()
        .map_err(|_| RemoteError::bad_request("Invalid diff cursor"))?;
    let dto = super::remote_bridge::file_diff_dto(&normalized, &file.diff, "snapshot");
    let lines=dto.hunks.iter().flat_map(|h|h.lines.iter().map(move|l|json!({"header":h.header,"origin":l.origin,"old_no":l.old_no,"new_no":l.new_no,"text":l.text}))).collect::<Vec<_>>();
    Ok(
        json!({"path":path,"source":"receipt","turn_id":turn,"is_binary":dto.is_binary,"lines":lines.iter().skip(offset).take(200).collect::<Vec<_>>(),"next_cursor":(offset+200<lines.len()).then(||(offset+200).to_string())}),
    )
}
