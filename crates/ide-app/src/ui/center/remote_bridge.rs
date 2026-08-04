use super::*;

use crate::remote::dto::{
    AccessModeConfigurationDto, AgentListItemDto, AgentSnapshotDto, ApprovalDecisionDto,
    ChangedFileDto, CommandAcceptedResponse, ConfigurationCatalogDto, DiffHunkDto, DiffLineDto,
    EffortConfigurationDto, FileDiffDto, InteractionModeDto, ModelConfigurationDto,
    PendingApprovalDto, PendingOptionDto, PendingQuestionDto, PendingUserInputDto, ProjectDto,
    ProviderConfigurationDto, ShipStateDto, TimelineItemDto, VerificationItemDto,
};
use crate::remote::{RemoteCommand, RemoteError, RemoteResult};
use crate::state::agent_chat::VerificationStatus;

impl CenterArea {
    pub(crate) fn handle_remote_command(&mut self, command: RemoteCommand, cx: &mut Context<Self>) {
        match command {
            RemoteCommand::GetConfiguration { response } => {
                let _ = response.send(Ok(remote_configuration_catalog()));
            }
            RemoteCommand::ListProjects { response } => {
                let _ = response.send(Ok(self.remote_projects(cx)));
            }
            RemoteCommand::ListAgents {
                project_id,
                response,
            } => {
                let result = parse_project_id(&project_id)
                    .and_then(|project_id| self.remote_agents(project_id, cx));
                let _ = response.send(result);
            }
            RemoteCommand::GetAgent { agent_id, response } => {
                let result = parse_agent_id(&agent_id)
                    .and_then(|agent_id| self.remote_agent_snapshot(agent_id, cx));
                let _ = response.send(result);
            }
            RemoteCommand::CreateAgent { request, response } => {
                let result = self.remote_create_agent(request, cx);
                let _ = response.send(result);
            }
            RemoteCommand::SendMessage {
                agent_id,
                request,
                response,
            } => {
                let result = parse_agent_id(&agent_id).and_then(|agent_id| {
                    let text = request.text.trim();
                    if text.is_empty() {
                        return Err(RemoteError::bad_request("message text cannot be empty"));
                    }
                    if !self.accept_remote_command_id(&request.client_command_id) {
                        return Ok(CommandAcceptedResponse { accepted: true });
                    }
                    if self.agents.read(cx).agent(agent_id).is_none() {
                        return Err(RemoteError::not_found("agent not found"));
                    }
                    let mode = interaction_mode(request.interaction_mode);
                    self.agent_chats.update(cx, |chats, cx| {
                        let title = self
                            .agents
                            .read(cx)
                            .agent(agent_id)
                            .map(|agent| agent.title.clone())
                            .unwrap_or_else(|| "Agent".into());
                        chats.ensure_session(agent_id, title, cx).interaction_mode = mode;
                    });
                    self.dispatch_agent_chat_submission(agent_id, text.to_string(), mode, cx);
                    Ok(CommandAcceptedResponse { accepted: true })
                });
                let _ = response.send(result);
            }
            RemoteCommand::UpdateAgentConfiguration {
                agent_id,
                request,
                response,
            } => {
                let result = parse_agent_id(&agent_id).and_then(|agent_id| {
                    let agent = self
                        .agents
                        .read(cx)
                        .agent(agent_id)
                        .cloned()
                        .ok_or_else(|| RemoteError::not_found("agent not found"))?;
                    let model = parse_wire_value::<AgentModel>(&request.model, "model")?;
                    if !model.belongs_to(agent.provider) {
                        return Err(RemoteError::bad_request(
                            "model does not belong to this agent's provider",
                        ));
                    }
                    let effort = parse_wire_value::<AgentEffort>(&request.effort, "effort")?;
                    let effort = agent.normalize_effort_for_model(model, effort);
                    let access_mode =
                        parse_wire_value::<AgentAccessMode>(&request.access_mode, "access mode")?;
                    if agent.model != model || agent.effort != effort {
                        self.update_agent_chat_model_effort(agent_id, model, effort, cx);
                    }
                    if agent.access_mode != access_mode {
                        self.agents.update(cx, |agents, cx| {
                            agents.update_access_mode(agent_id, access_mode, cx);
                        });
                        self.agent_chats.update(cx, |chats, cx| {
                            chats.update_access_mode(agent_id, access_mode, cx);
                        });
                    }
                    self.remote_agent_snapshot(agent_id, cx)
                });
                let _ = response.send(result);
            }
            RemoteCommand::StopAgent { agent_id, response } => {
                let result = parse_agent_id(&agent_id).and_then(|agent_id| {
                    if self.agents.read(cx).agent(agent_id).is_none() {
                        return Err(RemoteError::not_found("agent not found"));
                    }
                    self.agent_chats
                        .update(cx, |chats, cx| chats.stop_backend(agent_id, cx));
                    Ok(CommandAcceptedResponse { accepted: true })
                });
                let _ = response.send(result);
            }
            RemoteCommand::AnswerQuestion {
                agent_id,
                request_id,
                request,
                response,
            } => {
                let result = parse_agent_id(&agent_id).and_then(|agent_id| {
                    if !self.accept_remote_command_id(&request.client_command_id) {
                        return Ok(CommandAcceptedResponse { accepted: true });
                    }
                    let pending = self
                        .agent_chats
                        .read(cx)
                        .session(agent_id)
                        .and_then(|session| session.pending_user_input.clone())
                        .ok_or_else(|| RemoteError::conflict("agent is not waiting for input"))?;
                    if pending.request_id != request_id {
                        return Err(RemoteError::conflict(
                            "question request is no longer active",
                        ));
                    }
                    if request.answers.len() != pending.questions.len()
                        || request
                            .answers
                            .iter()
                            .any(|answer| answer.trim().is_empty())
                    {
                        return Err(RemoteError::bad_request(
                            "provide one non-empty answer for every question",
                        ));
                    }
                    self.agent_chats.update(cx, |chats, cx| {
                        if let Some(session) = chats.sessions.get_mut(&agent_id) {
                            if let Some(pending) = session.pending_user_input.as_mut() {
                                pending.question_index = 0;
                            }
                        }
                        for (index, answer) in request.answers.iter().enumerate() {
                            chats.set_pending_user_input_custom_answer(
                                agent_id,
                                answer.trim().to_string(),
                                cx,
                            );
                            if index + 1 < request.answers.len() {
                                chats.next_pending_user_input_question(agent_id, cx);
                            }
                        }
                        chats.submit_pending_user_input(agent_id, cx);
                    });
                    // Remote answers arrive as plain strings; picks come back
                    // as option labels (multi-select comma-joined). Only
                    // genuinely typed answers feed the memory-proposal pass.
                    let pairs = pending
                        .questions
                        .iter()
                        .zip(request.answers.iter())
                        .filter(|(question, answer)| {
                            !answer.trim().split(", ").all(|part| {
                                question
                                    .options
                                    .iter()
                                    .any(|option| option.label == part.trim())
                            })
                        })
                        .map(|(question, answer)| {
                            (question.question.clone(), answer.trim().to_string())
                        })
                        .collect::<Vec<_>>();
                    self.maybe_propose_memory_from_question_answers(agent_id, pairs, cx);
                    Ok(CommandAcceptedResponse { accepted: true })
                });
                let _ = response.send(result);
            }
            RemoteCommand::ResolvePlan {
                agent_id,
                request,
                response,
            } => {
                let result = parse_agent_id(&agent_id).and_then(|agent_id| {
                    if !self.accept_remote_command_id(&request.client_command_id) {
                        return Ok(CommandAcceptedResponse { accepted: true });
                    }
                    if self.agents.read(cx).agent(agent_id).is_none() {
                        return Err(RemoteError::not_found("agent not found"));
                    }
                    let plan_markdown = self
                        .agent_chats
                        .read(cx)
                        .session(agent_id)
                        .and_then(|session| session.proposed_plan.as_ref())
                        .filter(|plan| plan.implemented_at.is_none())
                        .map(|plan| plan.markdown.clone());
                    let Some(plan_markdown) = plan_markdown else {
                        return Err(RemoteError::conflict(
                            "agent has no plan awaiting a decision",
                        ));
                    };
                    let feedback = request.feedback.trim().to_string();
                    let (submission_text, mode) = self
                        .agent_chats
                        .update(cx, |chats, cx| {
                            chats.resolve_proposed_plan_submission(agent_id, &feedback, cx)
                        })
                        .ok_or_else(|| {
                            RemoteError::conflict("agent has no plan awaiting a decision")
                        })?;
                    self.dispatch_agent_chat_submission(agent_id, submission_text, mode, cx);
                    // Phone-typed plan feedback feeds the same memory-proposal
                    // pass as desktop feedback; the card renders on desktop.
                    if !feedback.is_empty() {
                        self.maybe_propose_memory_from_plan_feedback(
                            agent_id,
                            feedback,
                            plan_markdown,
                            cx,
                        );
                    }
                    Ok(CommandAcceptedResponse { accepted: true })
                });
                let _ = response.send(result);
            }
            RemoteCommand::DismissPlan {
                agent_id,
                request,
                response,
            } => {
                let result = parse_agent_id(&agent_id).and_then(|agent_id| {
                    if !self.accept_remote_command_id(&request.client_command_id) {
                        return Ok(CommandAcceptedResponse { accepted: true });
                    }
                    let has_plan = self
                        .agent_chats
                        .read(cx)
                        .session(agent_id)
                        .and_then(|session| session.proposed_plan.as_ref())
                        .is_some_and(|plan| plan.implemented_at.is_none());
                    if !has_plan {
                        return Err(RemoteError::conflict(
                            "agent has no plan awaiting a decision",
                        ));
                    }
                    self.agent_chats
                        .update(cx, |chats, cx| chats.dismiss_proposed_plan(agent_id, cx));
                    Ok(CommandAcceptedResponse { accepted: true })
                });
                let _ = response.send(result);
            }
            RemoteCommand::ResolveApproval {
                agent_id,
                request_id,
                request,
                response,
            } => {
                let result = parse_agent_id(&agent_id).and_then(|agent_id| {
                    if !self.accept_remote_command_id(&request.client_command_id) {
                        return Ok(CommandAcceptedResponse { accepted: true });
                    }
                    let pending = self
                        .agent_chats
                        .read(cx)
                        .session(agent_id)
                        .and_then(|session| session.pending_approval.clone())
                        .ok_or_else(|| RemoteError::conflict("agent has no approval request"))?;
                    if pending.request_id != request_id {
                        return Err(RemoteError::conflict(
                            "approval request is no longer active",
                        ));
                    }
                    let approved = matches!(request.decision, ApprovalDecisionDto::Approve);
                    let resolved = self.agent_chats.update(cx, |chats, cx| {
                        chats.resolve_pending_approval(agent_id, &request_id, approved, cx)
                    });
                    if !resolved {
                        return Err(RemoteError::conflict(
                            "agent approval backend is no longer available",
                        ));
                    }
                    Ok(CommandAcceptedResponse { accepted: true })
                });
                let _ = response.send(result);
            }
            RemoteCommand::CaptureVisualization {
                agent_id,
                path,
                response,
            } => {
                let agent_id = match parse_agent_id(&agent_id) {
                    Ok(agent_id) => agent_id,
                    Err(error) => {
                        let _ = response.send(Err(error));
                        return;
                    }
                };
                let Some(agent) = self.agents.read(cx).agent(agent_id).cloned() else {
                    let _ = response.send(Err(RemoteError::not_found("agent not found")));
                    return;
                };
                let context = agent_chat_visualization::ChatVisualizationContext::for_agent(&agent);
                let resolved =
                    match agent_chat_visualization::resolve_visualization_file(&context, &path) {
                        Ok(path) => path,
                        Err(_) => {
                            let _ = response.send(Err(RemoteError::not_found(
                                "visualization is not available",
                            )));
                            return;
                        }
                    };
                let is_live = self
                    .active_chat_visualization
                    .as_ref()
                    .is_some_and(|active| active.agent_id == agent_id && active.path == resolved);
                if !is_live {
                    let _ = response.send(Err(RemoteError::conflict(
                        "visualization is not live on desktop",
                    )));
                    return;
                }
                self.web_host.update(cx, move |host, _| {
                    host.capture_active_visualization_image(move |result| {
                        let result = result.map_err(RemoteError::conflict);
                        let _ = response.send(result);
                    });
                });
            }
            RemoteCommand::RequestVerificationFix {
                agent_id,
                request,
                response,
            } => {
                let result = parse_agent_id(&agent_id).and_then(|agent_id| {
                    if !self.accept_remote_command_id(&request.client_command_id) {
                        return Ok(CommandAcceptedResponse { accepted: true });
                    }
                    if self.agents.read(cx).agent(agent_id).is_none() {
                        return Err(RemoteError::not_found("agent not found"));
                    }
                    let fixable = self
                        .agent_chats
                        .read(cx)
                        .session(agent_id)
                        .map(|session| fixable_verification_id(&session.timeline))
                        .unwrap_or(None);
                    if fixable.as_deref() != Some(request.verification_id.as_str()) {
                        return Err(RemoteError::conflict("verification is not awaiting fixes"));
                    }
                    self.request_agent_verification_fix(
                        agent_id,
                        request.verification_id.clone(),
                        cx,
                    );
                    Ok(CommandAcceptedResponse { accepted: true })
                });
                let _ = response.send(result);
            }
            RemoteCommand::GetShipPreview { agent_id, response } => {
                let result = parse_agent_id(&agent_id)
                    .and_then(|agent_id| self.remote_ship_preview(agent_id, cx));
                let _ = response.send(result);
            }
            RemoteCommand::ShipAgentWork {
                agent_id,
                request,
                response,
            } => {
                let result = parse_agent_id(&agent_id).and_then(|agent_id| {
                    if !self.accept_remote_command_id(&request.client_command_id) {
                        return Ok(CommandAcceptedResponse { accepted: true });
                    }
                    self.remote_ship_agent_work(agent_id, request, cx)
                        .map(|_| CommandAcceptedResponse { accepted: true })
                });
                let _ = response.send(result);
            }
            RemoteCommand::GetFileDiff {
                agent_id,
                path,
                response,
            } => {
                let agent_id = match parse_agent_id(&agent_id) {
                    Ok(agent_id) => agent_id,
                    Err(error) => {
                        let _ = response.send(Err(error));
                        return;
                    }
                };
                let Some(agent) = self
                    .agents
                    .read(cx)
                    .agent(agent_id)
                    .cloned()
                    .filter(|agent| !agent.hidden_doc_assistant)
                else {
                    let _ = response.send(Err(RemoteError::not_found("agent not found")));
                    return;
                };
                let snapshot_id = self
                    .agent_chats
                    .read(cx)
                    .session(agent_id)
                    .and_then(|session| {
                        session.changed_files.snapshot_id.or_else(|| {
                            session.timeline.iter().rev().find_map(|item| match item {
                                AgentChatTimelineItem::ChangedFiles(summary) => summary.snapshot_id,
                                _ => None,
                            })
                        })
                    });
                let repo = agent.runtime_path().to_path_buf();
                cx.background_executor()
                    .spawn(async move {
                        let _ = response.send(compute_remote_file_diff(&repo, snapshot_id, &path));
                    })
                    .detach();
            }
        }
    }

    fn remote_projects(&self, cx: &App) -> Vec<ProjectDto> {
        let agents = self.agents.read(cx).all_records();
        self.workspace
            .read(cx)
            .projects
            .iter()
            .map(|project| ProjectDto {
                id: project.id.0.to_string(),
                name: project.name.clone(),
                agent_count: agents
                    .iter()
                    .filter(|agent| agent.project_id == project.id && !agent.hidden_doc_assistant)
                    .count(),
            })
            .collect()
    }

    fn remote_agents(
        &self,
        project_id: ProjectId,
        cx: &App,
    ) -> RemoteResult<Vec<AgentListItemDto>> {
        if !self
            .workspace
            .read(cx)
            .projects
            .iter()
            .any(|project| project.id == project_id)
        {
            return Err(RemoteError::not_found("project not found"));
        }
        Ok(self
            .agents
            .read(cx)
            .records_for_project(project_id)
            .into_iter()
            .filter(|agent| !agent.hidden_doc_assistant)
            .map(|agent| self.remote_agent_list_item(&agent, cx))
            .collect())
    }

    fn remote_agent_snapshot(&self, agent_id: Uuid, cx: &App) -> RemoteResult<AgentSnapshotDto> {
        let agent = self
            .agents
            .read(cx)
            .agent(agent_id)
            .cloned()
            .filter(|agent| !agent.hidden_doc_assistant)
            .ok_or_else(|| RemoteError::not_found("agent not found"))?;
        let project_name = self
            .workspace
            .read(cx)
            .projects
            .iter()
            .find(|project| project.id == agent.project_id)
            .map(|project| project.name.clone())
            .unwrap_or_else(|| "Unknown project".into());

        let live_session = self.agent_chats.read(cx).session(agent_id).cloned();
        let hydration = if live_session
            .as_ref()
            .is_none_or(|session| session.timeline.is_empty())
        {
            Self::load_chat_session_hydration(&agent)
        } else {
            None
        };
        let timeline = live_session
            .as_ref()
            .filter(|session| !session.timeline.is_empty())
            .map(|session| session.timeline.clone())
            .or_else(|| hydration.map(|hydration| hydration.timeline))
            .unwrap_or_default();
        let pending_user_input = live_session
            .as_ref()
            .and_then(|session| session.pending_user_input.as_ref())
            .map(pending_input_dto);
        let pending_approval = live_session
            .as_ref()
            .and_then(|session| session.pending_approval.as_ref())
            .map(pending_approval_dto);
        let changed_files = live_session
            .as_ref()
            .map(|session| changed_files_dto(&session.changed_files.files))
            .filter(|files| !files.is_empty())
            .unwrap_or_else(|| {
                agent
                    .changed_files
                    .iter()
                    .map(|file| ChangedFileDto {
                        path: file.path.to_string_lossy().to_string(),
                        additions: file.additions,
                        deletions: file.deletions,
                    })
                    .collect()
            });

        let fixable_id = fixable_verification_id(&timeline);
        Ok(AgentSnapshotDto {
            agent: self.remote_agent_list_item(&agent, cx),
            project_name,
            interaction_mode: live_session
                .as_ref()
                .map(|session| interaction_mode_label(session.interaction_mode))
                .unwrap_or("default")
                .into(),
            timeline: timeline
                .iter()
                .filter_map(|item| timeline_item_dto(item, fixable_id.as_deref()))
                .collect(),
            pending_user_input,
            pending_approval,
            changed_files,
            ship: self.remote_ship_status.get(&agent_id).map(ship_state_dto),
        })
    }

    fn remote_agent_list_item(&self, agent: &AgentRecord, cx: &App) -> AgentListItemDto {
        let session = self.agent_chats.read(cx).session(agent.id).cloned();
        let (status, started_running_at, last_activity_at, needs_attention) = session
            .map(|session| {
                (
                    chat_status_label(session.status).to_string(),
                    session.started_running_at,
                    session.last_activity_at,
                    session.pending_user_input.is_some()
                        || session.pending_approval.is_some()
                        || matches!(session.status, AgentChatStatus::PlanReady),
                )
            })
            .unwrap_or_else(|| {
                (
                    if agent.started_at.is_some() {
                        "idle".to_string()
                    } else {
                        "not_started".to_string()
                    },
                    None,
                    agent.updated_at,
                    false,
                )
            });
        AgentListItemDto {
            id: agent.id.to_string(),
            project_id: agent.project_id.0.to_string(),
            title: agent.title.clone(),
            backend: agent.provider_label().to_ascii_lowercase(),
            model: agent.model_label().to_string(),
            model_id: wire_value(agent.model),
            effort: wire_value(agent.effort),
            access_mode: wire_value(agent.access_mode),
            status,
            started_running_at,
            last_activity_at,
            needs_attention,
            solo: agent.is_solo(),
            solo_branch: agent.solo_branch.clone(),
        }
    }

    fn remote_create_agent(
        &mut self,
        request: crate::remote::dto::CreateAgentRequest,
        cx: &mut Context<Self>,
    ) -> RemoteResult<AgentSnapshotDto> {
        let project_id = parse_project_id(&request.project_id)?;
        let prompt = request.prompt.trim().to_string();
        if prompt.is_empty() {
            return Err(RemoteError::bad_request("prompt cannot be empty"));
        }
        let project = self
            .workspace
            .read(cx)
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .cloned()
            .ok_or_else(|| RemoteError::not_found("project not found"))?;
        let title = request
            .title
            .as_deref()
            .map(str::trim)
            .filter(|title| !title.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| {
                prompt
                    .lines()
                    .next()
                    .unwrap_or("New agent")
                    .chars()
                    .take(72)
                    .collect()
            });
        let provider = request
            .provider
            .as_deref()
            .map(|value| parse_wire_value::<AgentKind>(value, "provider"))
            .transpose()?
            .unwrap_or(AgentKind::Codex);
        let model = request
            .model
            .as_deref()
            .map(|value| parse_wire_value::<AgentModel>(value, "model"))
            .transpose()?
            .unwrap_or_else(|| AgentModel::default_for(provider));
        if !model.belongs_to(provider) {
            return Err(RemoteError::bad_request(
                "model does not belong to the selected provider",
            ));
        }
        let effort = request
            .effort
            .as_deref()
            .map(|value| parse_wire_value::<AgentEffort>(value, "effort"))
            .transpose()?
            .map(|effort| model.normalize_effort(effort))
            .unwrap_or_else(|| model.default_effort());
        let access_mode = resolve_remote_access_mode(request.access_mode.as_deref())?;
        let repository_paths = self
            .git_states
            .read(cx)
            .repositories(project.id)
            .into_iter()
            .filter_map(|git| {
                let git = git.read(cx);
                git.is_repo.then(|| git.repo_path.clone())
            })
            .collect::<Vec<_>>();
        let repository_path = resolve_remote_repository_path(
            &project.path,
            request.repository_path.as_deref(),
            &repository_paths,
            request.solo,
        )
        .map_err(RemoteError::bad_request)?;
        let agent_id = self.agents.update(cx, |agents, cx| {
            agents.create_agent(
                project.id,
                project.path.clone(),
                repository_path,
                title.clone(),
                prompt,
                provider,
                AgentRuntimeKind::Chat,
                model,
                effort,
                access_mode,
                Vec::new(),
                None,
                Vec::new(),
                None,
                AgentStatus::Todo,
                cx,
            )
        });
        if request.solo {
            let branch = ide_core::lanes::solo_branch_name(&title, agent_id);
            self.agents.update(cx, |agents, cx| {
                agents.configure_solo(agent_id, branch, None, ide_core::LaneProfile::Full, cx);
            });
        }
        let mode = interaction_mode(request.interaction_mode);
        self.agent_chats.update(cx, |chats, cx| {
            let title = self
                .agents
                .read(cx)
                .agent(agent_id)
                .map(|agent| agent.title.clone())
                .unwrap_or_else(|| "Agent".into());
            chats.ensure_session(agent_id, title, cx).interaction_mode = mode;
        });
        let agent = self
            .agents
            .read(cx)
            .agent(agent_id)
            .cloned()
            .ok_or_else(|| RemoteError::internal("created agent disappeared"))?;
        // A Solo's start waits for its lane; the lane callback starts the
        // backend once the worktree is ready — same path as the desktop.
        let started = if agent.is_solo() {
            self.ensure_solo_lane_then_start(agent_id, CenterMode::Agents, cx)
        } else {
            self.start_chat_agent_in_mode(agent, CenterMode::Agents, cx)
        };
        if !started {
            return Err(RemoteError::internal("failed to start agent backend"));
        }
        self.remote_agent_snapshot(agent_id, cx)
    }

    fn accept_remote_command_id(&mut self, command_id: &str) -> bool {
        let command_id = command_id.trim();
        if command_id.is_empty() {
            return true;
        }
        if self.remote_command_ids.contains(command_id) {
            return false;
        }
        if self.remote_command_ids.len() >= 512 {
            self.remote_command_ids.clear();
        }
        self.remote_command_ids.insert(command_id.to_string());
        true
    }
}

fn remote_configuration_catalog() -> ConfigurationCatalogDto {
    ConfigurationCatalogDto {
        providers: [AgentKind::Codex, AgentKind::Claude]
            .into_iter()
            .map(|provider| ProviderConfigurationDto {
                id: wire_value(provider),
                label: provider.label().to_string(),
                models: AgentModel::models_for(provider)
                    .iter()
                    .copied()
                    .map(|model| ModelConfigurationDto {
                        id: wire_value(model),
                        label: model.label().to_string(),
                        short_label: model.short_label().to_string(),
                        efforts: model
                            .efforts()
                            .into_iter()
                            .map(|effort| EffortConfigurationDto {
                                id: wire_value(effort),
                                label: effort.label().to_string(),
                                description: effort.menu_label().to_string(),
                            })
                            .collect(),
                    })
                    .collect(),
                access_modes: AgentAccessMode::ALL
                    .into_iter()
                    .map(|access_mode| AccessModeConfigurationDto {
                        id: wire_value(access_mode),
                        label: access_mode.label_for(provider).to_string(),
                        description: access_mode.description_for(provider).to_string(),
                    })
                    .collect(),
            })
            .collect(),
    }
}

fn wire_value<T: serde::Serialize>(value: T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// Resolve the access mode for a remotely created agent.
///
/// Security-critical: a remote request that omits `access_mode` must never
/// silently escalate to `FullAccess`, which runs shell commands and edits files
/// with no approval prompt. `AgentAccessMode::default()` is `FullAccess`, so
/// relying on `unwrap_or_default()` here would let a Control-permission phone
/// obtain unattended execution simply by leaving the field out. An omitted mode
/// resolves to the most restrictive, approval-gated mode instead; escalating to
/// FullAccess still requires an explicit request, which the transport-layer
/// permission guards reject unless the device has FullAccess permission.
fn resolve_remote_access_mode(value: Option<&str>) -> RemoteResult<AgentAccessMode> {
    match value {
        Some(raw) => parse_wire_value::<AgentAccessMode>(raw, "access mode"),
        None => Ok(AgentAccessMode::AskForApproval),
    }
}

fn parse_wire_value<T: serde::de::DeserializeOwned>(value: &str, field: &str) -> RemoteResult<T> {
    serde_json::from_value(serde_json::Value::String(value.trim().to_string()))
        .map_err(|_| RemoteError::bad_request(format!("invalid {field}")))
}

fn parse_project_id(value: &str) -> RemoteResult<ProjectId> {
    Uuid::parse_str(value)
        .map(ProjectId)
        .map_err(|_| RemoteError::bad_request("invalid project id"))
}

fn parse_agent_id(value: &str) -> RemoteResult<Uuid> {
    Uuid::parse_str(value).map_err(|_| RemoteError::bad_request("invalid agent id"))
}

fn interaction_mode(mode: InteractionModeDto) -> AgentInteractionMode {
    match mode {
        InteractionModeDto::Default => AgentInteractionMode::Default,
        InteractionModeDto::Plan => AgentInteractionMode::Plan,
    }
}

fn interaction_mode_label(mode: AgentInteractionMode) -> &'static str {
    match mode {
        AgentInteractionMode::Default => "default",
        AgentInteractionMode::Plan => "plan",
    }
}

fn chat_status_label(status: AgentChatStatus) -> &'static str {
    match status {
        AgentChatStatus::Idle => "idle",
        AgentChatStatus::Running => "running",
        AgentChatStatus::Cancelling => "cancelling",
        AgentChatStatus::WaitingForUser => "waiting_for_user",
        AgentChatStatus::PlanReady => "plan_ready",
        AgentChatStatus::Failed => "failed",
    }
}

/// The id of the one verification "Ask to fix" currently applies to: the
/// latest verification, only while the lifecycle is waiting on fixes and it
/// still has unmet items with no fix requested.
fn fixable_verification_id(timeline: &[AgentChatTimelineItem]) -> Option<String> {
    if agent_chat_runtime::verification_lifecycle(timeline)
        != agent_chat_runtime::VerificationLifecycle::NeedsFix
    {
        return None;
    }
    for item in timeline.iter().rev() {
        if let AgentChatTimelineItem::Verification(verification) = item {
            return verification
                .has_pending_fixes()
                .then(|| verification.id.clone());
        }
    }
    None
}

fn ship_state_dto(status: &agent_panel::RemoteShipStatus) -> ShipStateDto {
    match status {
        agent_panel::RemoteShipStatus::Shipping { started_at } => ShipStateDto {
            state: "shipping".into(),
            message: None,
            started_at: *started_at,
        },
        agent_panel::RemoteShipStatus::Failed { message, at } => ShipStateDto {
            state: "failed".into(),
            message: Some(message.clone()),
            started_at: *at,
        },
    }
}

fn verification_status_wire(status: VerificationStatus) -> &'static str {
    match status {
        VerificationStatus::Met => "met",
        VerificationStatus::Unclear => "unclear",
        VerificationStatus::Missed => "missed",
    }
}

fn timeline_item_dto(
    item: &AgentChatTimelineItem,
    fixable_verification_id: Option<&str>,
) -> Option<TimelineItemDto> {
    match item {
        AgentChatTimelineItem::Message(AgentChatMessage::User {
            text, created_at, ..
        }) => Some(TimelineItemDto::Message {
            role: "user".into(),
            text: text.clone(),
            created_at: *created_at,
        }),
        AgentChatTimelineItem::Message(AgentChatMessage::Assistant {
            text, created_at, ..
        }) => Some(TimelineItemDto::Message {
            role: "assistant".into(),
            text: text.clone(),
            created_at: *created_at,
        }),
        AgentChatTimelineItem::Message(AgentChatMessage::Thought {
            text, created_at, ..
        }) => Some(TimelineItemDto::Message {
            role: "thought".into(),
            text: text.clone(),
            created_at: *created_at,
        }),
        AgentChatTimelineItem::WorkLog(entry) => Some(TimelineItemDto::WorkLog {
            title: entry.title.clone(),
            detail: entry.detail.clone(),
            status: format!("{:?}", entry.status).to_ascii_lowercase(),
            count: entry.count.max(1),
            updated_at: entry.updated_at,
        }),
        AgentChatTimelineItem::PendingUserInput(_) => None,
        AgentChatTimelineItem::ProposedPlan(plan) => Some(TimelineItemDto::ProposedPlan {
            markdown: plan.markdown.clone(),
            implemented: plan.implemented_at.is_some(),
        }),
        AgentChatTimelineItem::CodeReview(review) => Some(TimelineItemDto::CodeReview {
            markdown: review.markdown.clone(),
        }),
        AgentChatTimelineItem::Verification(verification) => Some(TimelineItemDto::Verification {
            id: verification.id.clone(),
            markdown: verification.display_markdown(),
            met: verification.met_count(),
            total: verification.items.len(),
            fixable: fixable_verification_id == Some(verification.id.as_str()),
            items: verification
                .items
                .iter()
                .map(|item| VerificationItemDto {
                    status: verification_status_wire(item.status).to_string(),
                    title: item.title.clone(),
                    detail: item.detail.clone(),
                    fix_requested: item.fix_requested,
                })
                .collect(),
        }),
        AgentChatTimelineItem::ChangedFiles(summary) => Some(TimelineItemDto::ChangedFiles {
            files: changed_files_dto(&summary.files),
        }),
        AgentChatTimelineItem::ShipResult(result) => Some(TimelineItemDto::ShipResult {
            action: result.action.clone(),
            repository: result.repository.clone(),
            branch: result.branch.clone(),
            commit_sha: result.commit_sha.clone(),
            pr_url: result.pr_url.clone(),
            pr_title: result.pr_title.clone(),
            created_at: result.created_at,
        }),
        AgentChatTimelineItem::Rejoined(card) => Some(TimelineItemDto::Notice {
            title: format!("Rejoined {} into {}", card.branch, card.base),
            detail: None,
            created_at: card.created_at,
        }),
        AgentChatTimelineItem::RejoinConflict(card) => Some(TimelineItemDto::Notice {
            title: format!("Rejoin paused — conflicts with {}", card.target),
            detail: (!card.files.is_empty()).then(|| card.files.join(", ")),
            created_at: card.created_at,
        }),
        AgentChatTimelineItem::Memorized(card) => Some(TimelineItemDto::Notice {
            title: "Memorized".to_string(),
            detail: Some(card.text.clone()),
            created_at: card.created_at,
        }),
        // Desktop-only for now: the proposal card needs accept/dismiss
        // actions the remote protocol doesn't carry yet.
        AgentChatTimelineItem::MemoryProposal(_) => None,
    }
}

/// Resolve one file's diff for the phone: live worktree changes first, then
/// the diff snapshot captured at ship time so the file stays reviewable after
/// its changes were committed.
fn compute_remote_file_diff(
    repo: &std::path::Path,
    snapshot_id: Option<Uuid>,
    query: &str,
) -> RemoteResult<FileDiffDto> {
    let normalized_query = normalize_remote_diff_path(repo, std::path::Path::new(query));
    if let Ok(diffs) = ide_core::git::worktree_diffs(repo) {
        if let Some(diff) = diffs
            .into_iter()
            .find(|diff| normalize_remote_diff_path(repo, &diff.path) == normalized_query)
        {
            return Ok(file_diff_dto(&normalized_query, &diff, "worktree"));
        }
    }
    if let Some(snapshot_id) = snapshot_id {
        if let Ok(store) = ide_core::local_store::LocalStore::open_default() {
            if let Ok(Some(snapshot)) = store.load_agent_diff_snapshot(snapshot_id) {
                if let Some(file) = snapshot
                    .files
                    .into_iter()
                    .find(|file| normalize_remote_diff_path(repo, &file.path) == normalized_query)
                {
                    return Ok(file_diff_dto(&normalized_query, &file.diff, "snapshot"));
                }
            }
        }
    }
    Err(RemoteError::not_found("no diff available for this file"))
}

fn file_diff_dto(
    path: &std::path::Path,
    diff: &ide_core::git::FileDiff,
    source: &str,
) -> FileDiffDto {
    use ide_core::git::LineOrigin;
    let mut additions = 0;
    let mut deletions = 0;
    let hunks = diff
        .hunks
        .iter()
        .map(|hunk| DiffHunkDto {
            header: hunk.header.clone(),
            lines: hunk
                .lines
                .iter()
                .map(|line| {
                    match line.origin {
                        LineOrigin::Add => additions += 1,
                        LineOrigin::Remove => deletions += 1,
                        LineOrigin::Context => {}
                    }
                    DiffLineDto {
                        origin: match line.origin {
                            LineOrigin::Add => "add",
                            LineOrigin::Remove => "remove",
                            LineOrigin::Context => "context",
                        }
                        .to_string(),
                        old_no: line.old_no,
                        new_no: line.new_no,
                        text: line.text.trim_end_matches('\n').to_string(),
                    }
                })
                .collect(),
        })
        .collect();
    FileDiffDto {
        path: path.to_string_lossy().to_string(),
        is_binary: diff.is_binary,
        source: source.to_string(),
        additions,
        deletions,
        hunks,
    }
}

/// Repo-relative, separator-normalized form used to match a phone-supplied
/// path against diff paths regardless of absolute/relative origin.
fn normalize_remote_diff_path(
    repo: &std::path::Path,
    path: &std::path::Path,
) -> std::path::PathBuf {
    let relative = if path.is_absolute() {
        path.strip_prefix(repo).unwrap_or(path)
    } else {
        path
    };
    relative
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => Some(std::path::PathBuf::from(value)),
            _ => None,
        })
        .fold(std::path::PathBuf::new(), |mut acc, component| {
            acc.push(component);
            acc
        })
}

fn resolve_remote_repository_path(
    project_path: &std::path::Path,
    requested: Option<&str>,
    repository_paths: &[std::path::PathBuf],
    solo: bool,
) -> Result<Option<std::path::PathBuf>, String> {
    if let Some(requested) = requested {
        let requested = requested.trim();
        if requested.is_empty() {
            return Err("repository_path cannot be empty".into());
        }
        let requested = std::path::PathBuf::from(requested);
        let candidate = if requested.is_absolute() {
            requested
        } else {
            project_path.join(requested)
        };
        return repository_paths
            .iter()
            .find(|repository| **repository == candidate)
            .cloned()
            .map(Some)
            .ok_or_else(|| {
                "repository_path must identify a discovered Git repository inside the project"
                    .to_string()
            });
    }

    if !solo {
        return Ok(None);
    }
    match repository_paths {
        [] => Err("Solo needs a Git repository with at least one commit".into()),
        [repository] => Ok(Some(repository.clone())),
        _ => Err("repository_path is required for Solo in a multi-repository project".into()),
    }
}

fn changed_files_dto(files: &[crate::state::agent_chat::FileChangeStat]) -> Vec<ChangedFileDto> {
    files
        .iter()
        .map(|file| ChangedFileDto {
            path: file.path.to_string_lossy().to_string(),
            additions: file.additions,
            deletions: file.deletions,
        })
        .collect()
}

fn pending_input_dto(pending: &crate::state::agent_chat::PendingUserInput) -> PendingUserInputDto {
    PendingUserInputDto {
        request_id: pending.request_id.clone(),
        question_index: pending.question_index,
        questions: pending
            .questions
            .iter()
            .map(|question| PendingQuestionDto {
                id: question.id.clone(),
                header: question.header.clone(),
                question: question.question.clone(),
                options: question
                    .options
                    .iter()
                    .map(|option| PendingOptionDto {
                        label: option.label.clone(),
                        description: option.description.clone(),
                    })
                    .collect(),
                multi_select: question.multi_select,
            })
            .collect(),
    }
}

fn pending_approval_dto(pending: &crate::state::agent_chat::PendingApproval) -> PendingApprovalDto {
    PendingApprovalDto {
        request_id: pending.request_id.clone(),
        kind: pending.kind.wire_name().to_string(),
        title: pending.title.clone(),
        detail: pending.detail.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_ids_are_bad_requests() {
        assert_eq!(parse_agent_id("not-a-uuid").unwrap_err().status, 400);
        assert_eq!(parse_project_id("not-a-uuid").unwrap_err().status, 400);
    }

    #[test]
    fn omitted_remote_access_mode_never_escalates_to_full_access() {
        // A remote request that leaves out access_mode must not become a
        // no-approval FullAccess agent (unattended RCE). It resolves to the
        // most restrictive, approval-gated mode.
        assert_eq!(
            resolve_remote_access_mode(None).unwrap(),
            AgentAccessMode::AskForApproval,
        );
        assert_ne!(
            resolve_remote_access_mode(None).unwrap(),
            AgentAccessMode::FullAccess,
        );
    }

    #[test]
    fn explicit_remote_access_modes_are_preserved() {
        assert_eq!(
            resolve_remote_access_mode(Some("ask_for_approval")).unwrap(),
            AgentAccessMode::AskForApproval,
        );
        assert_eq!(
            resolve_remote_access_mode(Some("auto_accept_edits")).unwrap(),
            AgentAccessMode::AutoAcceptEdits,
        );
        assert_eq!(
            resolve_remote_access_mode(Some("full_access")).unwrap(),
            AgentAccessMode::FullAccess,
        );
    }

    #[test]
    fn invalid_remote_access_mode_is_a_bad_request() {
        assert_eq!(
            resolve_remote_access_mode(Some("root")).unwrap_err().status,
            400,
        );
    }

    #[test]
    fn remote_solo_requires_repository_when_project_has_many() {
        let project = std::path::Path::new("/workspace");
        let repositories = vec![project.join("backend"), project.join("frontend")];
        assert!(resolve_remote_repository_path(project, None, &repositories, true).is_err());
        assert_eq!(
            resolve_remote_repository_path(project, Some("frontend"), &repositories, true).unwrap(),
            Some(project.join("frontend")),
        );
    }

    #[test]
    fn remote_workspace_and_single_repo_solo_defaults_are_deterministic() {
        let project = std::path::Path::new("/workspace");
        let repositories = vec![project.join("frontend")];
        assert_eq!(
            resolve_remote_repository_path(project, None, &repositories, false).unwrap(),
            None,
        );
        assert_eq!(
            resolve_remote_repository_path(project, None, &repositories, true).unwrap(),
            Some(project.join("frontend")),
        );
    }
}
