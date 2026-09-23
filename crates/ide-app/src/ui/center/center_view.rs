use super::*;

impl CenterArea {
    pub fn view(
        workspace: Entity<Workspace>,
        terminals: Entity<TerminalManager>,
        agents: Entity<AgentRecords>,
        agent_chats: Entity<AgentChatState>,
        agent_activity: Entity<AgentActivityCache>,
        git_states: Entity<GitStates>,
        docs: Entity<DocsState>,
        designs: Entity<DesignsState>,
        tasks: Entity<TasksState>,
        services: Entity<ServicesState>,
        orbit: Entity<OrbitState>,
        doc_assistants: Entity<DocAssistantState>,
        voice: Entity<VoiceState>,
        quick_ask: Entity<QuickAskState>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        let composer_branch_query =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search branches"));
        let composer_model_query =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search models or providers"));
        let orbit_search = cx.new(|cx| InputState::new(window, cx).placeholder("Search records"));
        let services_env_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search variables"));
        let quick_ask_history_search = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Search questions, answers, or projects")
        });
        let simulator_bridge = Arc::new(parking_lot::Mutex::new(
            ios_simulator_preview::SimulatorBridgeController::default(),
        ));
        let simulator_bridge_task = simulator_bridge.clone();
        let (web_wake, web_messages) = async_channel::bounded(1);
        let web_app = web_preview::WebPreviewWake::new(web_wake);
        let native_wake = web_app.clone();
        let compare_web_app = web_app.clone();
        let web_window = window.window_handle();
        let web_host = cx.new(|_| web_preview::WebPreviewHost::new(web_app));
        let compare_web_host = cx.new(|_| web_preview::WebPreviewHost::new(compare_web_app));
        let preview_control_root = ide_core::AppConfig::config_path()
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let (project_preview_control_server, preview_control_receiver) =
            match preview_control_ipc::PreviewControlServer::start(preview_control_root) {
                Ok((server, receiver)) => (Some(server), receiver),
                Err(error) => {
                    eprintln!("could not start Preview control IPC: {error:#}");
                    let (sender, receiver) = async_channel::bounded(1);
                    drop(sender);
                    (None, receiver)
                }
            };
        // Baseline already-loaded agents before subscribing to changes. Without
        // this, the first status event can itself be the transition to Done and
        // terminal summary maintenance has no previous state to compare.
        let agent_record_status_seen = agents
            .read(cx)
            .all_records()
            .into_iter()
            .map(|agent| (agent.id, agent.status))
            .collect();
        let center = cx.new(move |cx| {
            cx.observe(&cx.entity(), |this: &mut Self, _, cx| {
                this.native_surface_dirty = true;
                this.sync_studio_regions(cx);
            }).detach();
            cx.observe_global::<gpui_component::Theme>(|_: &mut Self, cx| cx.notify()).detach();
            let project_navigation =
                center_navigation::ProjectNavigation::new(workspace.read(cx).active);
            cx.observe(&workspace, |this: &mut Self, _, cx| {
                this.sync_project_navigation(cx);
                this.refresh_open_code_models(false, cx);
                this.reconcile_verification_mode(cx);
                cx.notify();
            })
            .detach();
            cx.observe(&terminals, |_, _, cx| cx.notify()).detach();
            cx.subscribe(&agents, |this: &mut Self, _, event, cx| {
                if let crate::state::agents::AgentRecordsEvent::RecordChanged { agent_id, .. } = event {
                    if this.rendered_chat_agents.contains(agent_id)
                        || (this.view_mode == CenterMode::Agents && this.rendered_chat_agents.is_empty())
                        || (this.view_mode == CenterMode::Design && this.studio.as_ref().is_some_and(|s| s.implementation_agents.contains(agent_id))) {
                        cx.notify();
                    }
                }
            }).detach();
            // Transcript freshness drives the chat's working indicator when the
            // session status has gone stale, so a cache refresh must repaint.
            cx.subscribe(&agent_activity, |this: &mut Self, _, change, cx| {
                if change.0.iter().any(|id| this.rendered_chat_agents.contains(id)) {
                    cx.notify();
                }
            }).detach();
            cx.observe(&quick_ask, |_, _, cx| cx.notify()).detach();
            cx.observe(&quick_ask_history_search, |_, _, cx| cx.notify())
                .detach();
            cx.subscribe(
                &agents,
                |this: &mut Self, _, event: &crate::state::agents::AgentRecordsEvent, cx| {
                    if matches!(event, crate::state::agents::AgentRecordsEvent::Changed) {
                        this.maybe_terminal_agent_summary(cx);
                    }
                    if matches!(
                        event,
                        crate::state::agents::AgentRecordsEvent::SelectionChanged
                    ) {
                        let open_projects = this
                            .project_preview_ui
                            .iter()
                            .filter_map(|(project, ui)| ui.open.then_some(*project))
                            .collect::<Vec<_>>();
                        for project in open_projects {
                            this.reconcile_project_preview_for_selected_agent(project, cx);
                        }
                        cx.notify();
                    }
                },
            )
            .detach();
            cx.subscribe(&agent_chats, |this: &mut Self, _, event, cx| {
                let AgentChatEvent::SessionChanged(change) = event else { return; };
                this.agent_chat_render_sessions.remove(&change.agent_id);
                if change.categories.identity || change.categories.controls {
                    this.sync_chat_session_ids_for(Some(change.agent_id), cx);
                    this.sync_doc_assistant_chat_session_ids(cx);
                }
                if change.categories.navigation || change.categories.controls {
                    this.maybe_finalize_doc_assistant_titles(cx);
                    this.maybe_auto_verify(cx);
                    this.maybe_finish_summary_maintenance(cx);
                    this.maybe_start_pending_pocketcomet_handoffs(cx);
                }
                let affects_parent = (change.categories.navigation || change.categories.identity)
                    && this.agents.read(cx).agent(change.agent_id)
                        .and_then(|agent| agent.delegation.as_ref())
                        .is_some_and(|binding| this.rendered_chat_agents.contains(&binding.parent_agent_id));
                if this.rendered_chat_agents.contains(&change.agent_id) || affects_parent {
                    this.schedule_agent_chat_search_live_refresh(cx);
                    cx.notify();
                } else if change.categories.navigation || change.categories.identity {
                    // Agent listings and Band overviews show status without a transcript.
                    if this.view_mode == CenterMode::Agents && this.rendered_chat_agents.is_empty() { cx.notify(); }
                }
            }).detach();
            cx.subscribe(
                &agent_chats,
                |this: &mut Self, _, event: &AgentChatEvent, cx| match event {
                    AgentChatEvent::TurnFinished {
                        agent_id,
                        source_turn_id,
                    } => {
                        this.complete_orbit_invocation_for_agent(*agent_id, cx);
                        let review_started = this.maybe_request_review_checklist(
                            *agent_id,
                            source_turn_id.clone(),
                            cx,
                        );
                        this.schedule_pocketcomet_handoff(*agent_id, review_started, cx);
                    }
                    AgentChatEvent::WorkFinished { agent_id } => {
                        // Some remote runtimes settle work without emitting a
                        // local TurnFinished event. Completion is idempotent:
                        // the first terminal event removes the active grant.
                        this.complete_orbit_invocation_for_agent(*agent_id, cx);
                        if let Err(error) = this.workspace.update(cx, |workspace, cx| {
                            workspace.refresh_project_presets_from_store(cx)
                        }) {
                            eprintln!(
                                "could not refresh Choro scripts after agent work: {error:#}"
                            );
                        }
                        this.docs.update(cx, |docs, cx| docs.refresh(cx));
                        this.schedule_pocketcomet_handoff(*agent_id, false, cx);
                    }
                    AgentChatEvent::SessionChanged(_) => {}
                },
            )
            .detach();
            if let Some(handle) = cx
                .try_global::<crate::state::delegation::DelegationHandle>()
                .cloned()
            {
                cx.subscribe(&handle.0, |this: &mut Self, _, change, cx| {
                    let selected = this.workspace.read(cx).active.and_then(|project| this.agents.read(cx).selected_agent_id(project));
                    if change.parents.iter().any(|id| this.rendered_chat_agents.contains(id) || (this.view_mode == CenterMode::Agents && selected == Some(*id)))
                        || (change.error_changed && (this.delegated_overview.is_some() || this.delegated_panel.is_some())) {
                        cx.notify();
                    }
                }).detach();
            }
            cx.observe(&docs, |_, _, cx| cx.notify()).detach();
            cx.subscribe(&docs, |this: &mut Self, _docs, event: &DocsEvent, cx| {
                if let DocsEvent::OpenReference { doc_path, target } = event {
                    this.pending_reference_open = Some((doc_path.clone(), target.clone()));
                    cx.notify();
                }
                if matches!(event, DocsEvent::Changed) {
                    this.schedule_solo_docs_refresh(cx);
                }
            })
            .detach();
            cx.observe(&designs, |_, _, cx| cx.notify()).detach();
            cx.observe(&tasks, |_, _, cx| cx.notify()).detach();
            cx.observe(&services, |_, _, cx| cx.notify()).detach();
            cx.observe(&orbit, |_, _, cx| cx.notify()).detach();
            cx.subscribe(&orbit, |this: &mut Self, _, event: &OrbitEvent, cx| {
                if let OrbitEvent::RecordSaved {
                    project,
                    module_id,
                    request_id,
                    ..
                } = event
                {
                    if this.orbit_record_editor.as_ref().is_some_and(|editor| {
                        editor.project == *project
                            && editor.module_id == *module_id
                            && editor.request_id == *request_id
                    }) {
                        let window_handle = this
                            .orbit_record_editor
                            .as_ref()
                            .map(|editor| editor.window_handle);
                        this.orbit_record_editor = None;
                        if let Some(window_handle) = window_handle {
                            window_handle
                                .update(cx, |_, window, cx| window.close_dialog(cx))
                                .ok();
                        }
                        cx.notify();
                    }
                }
            })
            .detach();
            cx.observe(&doc_assistants, |_, _, cx| cx.notify()).detach();
            cx.subscribe(&composer_branch_query, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            })
            .detach();
            cx.subscribe(&composer_model_query, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            })
            .detach();
            cx.subscribe(&orbit_search, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            })
            .detach();
            cx.subscribe(&services_env_search, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            })
            .detach();
            cx.spawn(async move |this, cx| loop {
                let Some(center) = this.upgrade() else {
                    break;
                };
                let poll = center
                    .update(cx, |this: &mut Self, cx| {
                        let paths = this
                            .terminals
                            .read(cx)
                            .sessions
                            .iter()
                            .filter(|session| {
                                session.doc_assistant_key.is_some() && !session.exited
                            })
                            .map(|session| session.cwd.clone())
                            .collect::<Vec<_>>();
                        if paths.is_empty() {
                            None
                        } else {
                            let mut paths = paths;
                            paths.sort();
                            paths.dedup();
                            Some(paths)
                        }
                    })
                    .ok()
                    .flatten();
                if let Some(paths) = poll {
                    let chats_by_cwd: HashMap<PathBuf, Vec<ide_core::AgentChat>> = cx
                        .background_executor()
                        .spawn(async move {
                            paths
                                .into_iter()
                                .map(|path| {
                                    let chats = ide_core::agents::list_chats(&path);
                                    (path, chats)
                                })
                                .collect()
                        })
                        .await;
                    center
                        .update(cx, |this: &mut Self, cx| {
                            let adopted = this.terminals.update(cx, |terminals, cx| {
                                terminals.adopt_doc_assistant_ids(&chats_by_cwd, cx)
                            });
                            if !adopted.is_empty() {
                                let session_updates = {
                                    let terminals = this.terminals.read(cx);
                                    adopted
                                        .iter()
                                        .filter_map(|(key, session_id)| {
                                            terminals
                                                .sessions
                                                .iter()
                                                .find(|session| {
                                                    session.doc_assistant_key.as_deref()
                                                        == Some(key.as_str())
                                                        && session.agent_session_id.as_deref()
                                                            == Some(session_id.as_str())
                                                })
                                                .and_then(|session| {
                                                    let kind = session.agent?;
                                                    let transcript =
                                                        ide_core::agents::chat_transcript_path(
                                                            kind,
                                                            &session.cwd,
                                                            session_id,
                                                        );
                                                    Some((
                                                        key.clone(),
                                                        session_id.clone(),
                                                        transcript,
                                                    ))
                                                })
                                        })
                                        .collect::<Vec<_>>()
                                };
                                this.doc_assistants.update(cx, |assistants, cx| {
                                    for (key, session_id, transcript) in session_updates {
                                        assistants
                                            .set_cli_session_id(&key, session_id, transcript, cx);
                                    }
                                });
                            }

                            let proposal_updates = {
                                let terminals = this.terminals.read(cx);
                                terminals
                                    .sessions
                                    .iter()
                                    .filter_map(|session| {
                                        let key = session.doc_assistant_key.clone()?;
                                        let kind = session.agent?;
                                        let session_id = session.agent_session_id.as_deref()?;
                                        let messages = doc_assistant::read_chat_messages(
                                            kind,
                                            &session.cwd,
                                            session_id,
                                        );
                                        let proposal = latest_doc_proposal(&messages);
                                        Some((key, proposal))
                                    })
                                    .collect::<Vec<_>>()
                            };
                            if !proposal_updates.is_empty() {
                                this.doc_assistants.update(cx, |assistants, cx| {
                                    for (key, proposal) in proposal_updates {
                                        assistants.set_pending_proposal_by_key(&key, proposal, cx);
                                    }
                                });
                            }
                            cx.notify();
                        })
                        .ok();
                }
                center
                    .update(cx, |this: &mut Self, cx| {
                        if this.view_mode == CenterMode::Docs {
                            this.docs.update(cx, |docs, cx| {
                                docs.refresh(cx);
                                docs.refresh_open_docs_from_disk(cx);
                            });
                        }
                    })
                    .ok();
                cx.background_executor()
                    .timer(DOC_ASSISTANT_REFRESH_INTERVAL)
                    .await;
            })
            .detach();
            cx.spawn(async move |this, cx| loop {
                cx.background_executor()
                    .timer(AGENT_SHIP_PR_REFRESH_INTERVAL)
                    .await;
                let Some(center) = this.upgrade() else {
                    break;
                };
                center.update(cx, |_, cx| cx.notify()).ok();
            })
            .detach();
            cx.spawn(async move |this, cx| loop {
                cx.background_executor()
                    .timer(AGENT_CHAT_IDLE_RETIRE_CHECK_INTERVAL)
                    .await;
                let Some(center) = this.upgrade() else {
                    break;
                };
                center
                    .update(cx, |this: &mut Self, cx| {
                        this.retire_idle_agent_chat_backends(cx)
                    })
                    .ok();
            })
            .detach();
            cx.spawn(async move |this, cx| {
                let mut preview_services = HashMap::new();
                loop {
                    let Some(center) = this.upgrade() else {
                        break;
                    };
                    let (project_ids, needs_brain_bootstrap) = center
                        .update(cx, |this: &mut Self, cx| {
                            (
                                this.workspace
                                    .read(cx)
                                    .projects
                                    .iter()
                                    .map(|project| project.id)
                                    .collect::<Vec<_>>(),
                                !this.brain_poll_bootstrapped,
                            )
                        })
                        .unwrap_or_default();
                    let (records, memories, summaries, agent_messages, store_opened) = cx
                        .background_executor()
                        .spawn(async move {
                            let Ok(store) = ide_core::local_store::LocalStore::open_default()
                            else {
                                return (HashMap::new(), Vec::new(), Vec::new(), Vec::new(), false);
                            };
                            if needs_brain_bootstrap {
                                if let Err(error) = store.clear_project_previews() {
                                    eprintln!(
                                        "could not clear transient Preview requests: {error:#}"
                                    );
                                }
                            }
                            let records = project_ids
                                .into_iter()
                                .map(|project| {
                                    let previews =
                                        store.load_project_previews(project).unwrap_or_default();
                                    (project, previews)
                                })
                                .collect::<HashMap<_, _>>();
                            // Same DB channel as previews: the MCP `memory_save`
                            // writes rows the GUI notices here.
                            let memories = store.load_all_memories().unwrap_or_default();
                            let summaries = store.load_all_agent_summaries().unwrap_or_default();
                            let agent_messages =
                                store.load_pending_agent_messages().unwrap_or_default();
                            (records, memories, summaries, agent_messages, true)
                        })
                        .await;
                    center
                        .update(cx, |this: &mut Self, cx| {
                            if needs_brain_bootstrap
                                && store_opened
                                && !this.brain_poll_bootstrapped
                            {
                                this.memory_card_ids_seen =
                                    memories.iter().map(|memory| memory.id).collect();
                                this.agent_summaries = summaries
                                    .iter()
                                    .cloned()
                                    .map(|summary| (summary.agent_id, summary))
                                    .collect();
                                this.brain_poll_bootstrapped = true;
                            } else {
                                this.surface_fresh_memorized_cards(&memories, cx);
                                this.surface_fresh_agent_summaries(&summaries, cx);
                            }
                            this.surface_pending_agent_messages(&agent_messages, cx);
                            this.maybe_check_rejoin_ready(cx);
                            let now = SystemTime::now()
                                .duration_since(SystemTime::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs();
                            let changed = this.project_preview_records != records;
                            for (project, previews) in &records {
                                let Some(latest) = previews.first() else {
                                    continue;
                                };
                                let previous = this
                                    .project_preview_latest_seen
                                    .insert(*project, latest.updated_at);
                                let newly_opened = previous
                                    .map(|previous| latest.updated_at > previous)
                                    .unwrap_or_else(|| now.saturating_sub(latest.updated_at) <= 5);
                                if newly_opened {
                                    this.project_preview_selected_urls
                                        .insert(*project, latest.url.clone());
                                    let ui = this.project_preview_ui.entry(*project).or_default();
                                    ui.open = true;
                                    ui.status = Some(format!(
                                        "Opened {}",
                                        latest
                                            .title
                                            .trim()
                                            .is_empty()
                                            .then_some("Preview")
                                            .unwrap_or(latest.title.as_str())
                                    ));
                                }
                            }
                            this.project_preview_records = records;
                            // PTY discovery changes shared terminal data without
                            // a model event. Compare its small presentation snapshot
                            // instead of invalidating chat history on every idle poll.
                            let services = this.project_preview_ui.iter().filter(|(_, ui)| ui.open)
                                .map(|(project, _)| (*project, this.terminals.read(cx).project_preview_services(*project)))
                                .collect::<HashMap<_, _>>();
                            let services_changed = preview_services != services;
                            preview_services = services;
                            if changed || services_changed { cx.notify(); }
                        })
                        .ok();
                    cx.background_executor()
                        .timer(PROJECT_PREVIEW_POLL_INTERVAL)
                        .await;
                }
            })
            .detach();
            // Native messages wake their handler even when no animation is mounted.
            // The timer maintains external activity/catalog recovery, not redraws.
            cx.spawn(async move |this, cx| {
                loop {
                    let timer = cx.background_executor().timer(Duration::from_millis(400));
                    let signal = futures_util::future::select(Box::pin(web_messages.recv()), Box::pin(timer)).await;
                    if matches!(signal, futures_util::future::Either::Left((Err(_), _))) { break; }
                    let result = web_window.update(cx, |_, window, cx| {
                        this.update(cx, |this, cx| {
                            let previous_studio = this.studio_region_key(true);
                            let general = native_wake.take_general();
                            this.process_native_messages(window, cx);
                            if let Some((project, _)) = this.active_project(cx) {
                                let effective_mode = this.view_mode;
                                if matches!(effective_mode, CenterMode::Design | CenterMode::Tasks | CenterMode::MyTasks | CenterMode::Docs | CenterMode::Agents)
                                    && this.studio_catalog_refreshed.get(&project).is_none_or(|last| last.elapsed() > Duration::from_secs(2)) {
                                    this.refresh_studio_catalog(project, cx);
                                }
                            }
                            // IPC updates state; only that state can request a redraw.
                            // A message may arrive just after the recovery timer wins
                            // select. Its flag still requires a redraw in this pass.
                            if general {
                                cx.notify();
                            } else if previous_studio != this.studio_region_key(true) {
                                this.native_surface_dirty = true;
                                if let Some(stage) = &this.studio_stage_view {
                                    // A canvas/control change dirties its region and the thin
                                    // layout shell, without broadcasting to chat history.
                                    stage.update(cx, |_, cx| cx.notify());
                                } else { cx.notify(); }
                            }
                        })
                    });
                    if result.is_err() { break; }
                }
            }).detach();
            cx.spawn(async move |this, cx| {
                while let Ok(envelope) = preview_control_receiver.recv().await {
                    let Some(center) = this.upgrade() else {
                        break;
                    };
                    center
                        .update(cx, |this: &mut Self, cx| {
                            if envelope.request.action == ide_core::preview_control::DELEGATION_CHANGED_ACTION {
                                // Authentication and framing were checked by the IPC server.
                                // The payload is only a reload hint, never trusted state.
                                let valid = serde_json::from_str::<Uuid>(&envelope.request.payload_json).is_ok();
                                if valid {
                                    if let Some(handle) = cx.try_global::<crate::state::delegation::DelegationHandle>().cloned() {
                                        handle.0.update(cx, |coordinator, _| coordinator.request_reload());
                                    }
                                }
                                let response = if valid {
                                    ide_core::preview_control::PreviewControlResponse::success(envelope.request.id, "{}".into(), None)
                                } else {
                                    ide_core::preview_control::PreviewControlResponse::failure(envelope.request.id, "Invalid delegation hint")
                                };
                                let _ = envelope.respond_to.try_send(response);
                            } else {
                                this.enqueue_project_preview_control_command(envelope, cx);
                            }
                        })
                        .ok();
                }
            })
            .detach();
            cx.spawn(async move |this, cx| {
                let mut last_discovery =
                    Instant::now().checked_sub(IOS_SIMULATOR_DISCOVERY_INTERVAL);
                let mut bridge_retry_after: Option<(String, Instant)> = None;
                loop {
                    let Some(center) = this.upgrade() else {
                        break;
                    };
                    let active_preview_project = center
                        .update(cx, |this: &mut Self, cx| {
                            this.active_project(cx)
                                .map(|(project, _)| project)
                                .filter(|project| this.is_project_preview_open(*project))
                        })
                        .ok()
                        .flatten();

                    let should_discover = last_discovery
                        .is_none_or(|last| last.elapsed() >= IOS_SIMULATOR_DISCOVERY_INTERVAL)
                        && active_preview_project.is_some();
                    if should_discover {
                        let discovered = cx
                            .background_executor()
                            .spawn(async { ios_simulator_preview::discover_booted_simulators() })
                            .await;
                        last_discovery = Some(Instant::now());
                        if let Ok(discovered) = discovered {
                            center
                                .update(cx, |this: &mut Self, cx| {
                                    if this.ios_simulators != discovered {
                                        let active_udids = discovered
                                            .iter()
                                            .map(|device| device.udid.as_str())
                                            .collect::<HashSet<_>>();
                                        let disconnected_projects = this
                                            .project_preview_selected_urls
                                            .iter()
                                            .filter_map(|(project, source)| {
                                                let udid =
                                                    ios_simulator_preview::source_udid(source)?;
                                                (!active_udids.contains(udid)).then_some(*project)
                                            })
                                            .collect::<Vec<_>>();
                                        this.ios_simulators = discovered;
                                        for project in disconnected_projects {
                                            this.set_project_preview_status(
                                                project,
                                                Some("Simulator disconnected".to_string()),
                                            );
                                        }
                                        cx.notify();
                                    }
                                })
                                .ok();
                        }
                    }

                    let desired_bridge = center
                        .update(cx, |this: &mut Self, _cx| {
                            let project = active_preview_project?;
                            let selected = this.project_preview_selected_urls.get(&project)?;
                            let udid = ios_simulator_preview::source_udid(selected)?.to_string();
                            this.ios_simulators
                                .iter()
                                .any(|device| device.udid == udid)
                                .then_some((project, udid))
                        })
                        .ok()
                        .flatten();
                    let desired_udid = desired_bridge.as_ref().map(|(_, udid)| udid.clone());
                    let retry_blocked = desired_udid.as_ref().is_some_and(|udid| {
                        bridge_retry_after.as_ref().is_some_and(|(failed, retry)| {
                            failed == udid && Instant::now() < *retry
                        })
                    });
                    if !retry_blocked {
                        let bridge = simulator_bridge_task.clone();
                        let reconcile_udid = desired_udid.clone();
                        let reconciled = cx
                            .background_executor()
                            .spawn(
                                async move { bridge.lock().reconcile(reconcile_udid.as_deref()) },
                            )
                            .await;
                        match reconciled {
                            Ok(endpoint) => {
                                bridge_retry_after = None;
                                center
                                    .update(cx, |this: &mut Self, cx| {
                                        if this.simulator_bridge_endpoint != endpoint {
                                            this.simulator_bridge_endpoint = endpoint.clone();
                                            if let (Some((project, _)), Some(_)) =
                                                (&desired_bridge, &endpoint)
                                            {
                                                this.set_project_preview_status(
                                                    *project,
                                                    Some("Simulator connected".to_string()),
                                                );
                                            }
                                            cx.notify();
                                        }
                                        if endpoint.is_none() {
                                            this.simulator_preview_error_udid = None;
                                        }
                                    })
                                    .ok();
                            }
                            Err(error) => {
                                if let Some((project, udid)) = &desired_bridge {
                                    bridge_retry_after = Some((
                                        udid.clone(),
                                        Instant::now() + Duration::from_secs(5),
                                    ));
                                    center
                                        .update(cx, |this: &mut Self, cx| {
                                            this.simulator_bridge_endpoint = None;
                                            if this.simulator_preview_error_udid.as_deref()
                                                != Some(udid.as_str())
                                            {
                                                this.simulator_preview_error_udid =
                                                    Some(udid.clone());
                                                this.set_project_preview_status(
                                                    *project,
                                                    Some(format!(
                                                        "Simulator Preview unavailable: {error:#}"
                                                    )),
                                                );
                                                cx.notify();
                                            }
                                        })
                                        .ok();
                                }
                            }
                        }
                    }

                    cx.background_executor()
                        .timer(Duration::from_millis(500))
                        .await;
                }
            })
            .detach();
            Self {
                workspace,
                quick_ask,
                quick_ask_selected_session: None,
                quick_ask_history_search,
                quick_ask_history_composer: None,
                quick_ask_history_pending_started_at: None,
                weekly_digest_expanded: HashSet::new(),
                terminals,
                agents,
                agent_chats,
                agent_activity,
                git_states,
                docs,
                designs,
                tasks,
                services,
                orbit,
                orbit_search,
                services_env_search,
                services_env_filter: ServicesEnvFilter::All,
                orbit_table_scroll: ScrollHandle::new(),
                orbit_record_editor: None,
                orbit_collapsed_sections: HashSet::new(),
                orbit_active_invocations: HashMap::new(),
                orbit_pending_invocations: HashMap::new(),
                doc_assistants,
                voice,
                design_hub_error: None,
                design_compare_open: false,
                pending_studio_drafts: HashMap::new(),
                studio: None,
                studio_stage_view: None,
                studio_sidebar_view: None,
                studio_system_library: None,
                studio_code_imports: HashMap::new(),
                studio_system_catalog: HashMap::new(),
                studio_catalog: HashMap::new(),
                studio_catalog_refreshing: HashSet::new(),
                studio_catalog_refreshed: HashMap::new(),
                studio_catalog_implementors: HashMap::new(),
                studio_creating: HashSet::new(),
                studio_catalog_previews: HashMap::new(),
                figma_open_design: None,
                web_host,
                compare_web_host,
                editors: Vec::new(),
                diffs: Vec::new(),
                db_views: Vec::new(),
                agent_notes_inputs: HashMap::new(),
                agent_chat_inputs: HashMap::new(),
                agent_chat_attached_files: HashMap::new(),
                agent_chat_attachment_pastes_pending: HashMap::new(),
                agent_chat_pasted_text_blocks: HashMap::new(),
                agent_chat_selected_commands: HashMap::new(),
                delegation_selection: HashMap::new(),
                delegation_picker: None,
                delegated_panel: None,
                delegated_overview: None,
                delegated_preview: false,
                delegated_brief_expanded: HashSet::new(),
                agent_chat_capability_cache: RefCell::new(None),
                agent_chat_preview_armed: HashSet::new(),
                agent_chat_preview_suggestion_dismissed: HashSet::new(),
                project_preview_review_ids_seen: HashSet::new(),
                project_preview_ui: HashMap::new(),
                project_preview_panel_ratio: PROJECT_PREVIEW_PANEL_DEFAULT_RATIO,
                project_preview_available_width: 0.0,
                project_preview_resize: None,
                project_preview_selected_urls: HashMap::new(),
                project_preview_url_inputs: HashMap::new(),
                project_preview_project_urls: HashMap::new(),
                project_preview_solo_urls: HashMap::new(),
                project_preview_records: HashMap::new(),
                project_preview_latest_seen: HashMap::new(),
                project_preview_inspecting: None,
                project_preview_control_inflight: None,
                project_preview_control_queue: VecDeque::new(),
                project_preview_control_navigation_barrier: None,
                project_preview_control_server,
                ios_simulators: Vec::new(),
                simulator_bridge,
                simulator_bridge_endpoint: None,
                simulator_preview_error_udid: None,
                agent_chat_selected_mentions: HashMap::new(),
                agent_chat_slash_selection: HashMap::new(),
                agent_chat_slash_dismissed_query: HashMap::new(),
                agent_chat_doc_selection: HashMap::new(),
                agent_chat_doc_dismissed_query: HashMap::new(),
                agent_chat_file_selection: HashMap::new(),
                agent_chat_file_dismissed_query: HashMap::new(),
                agent_chat_selected_agent_targets: HashMap::new(),
                agent_chat_agent_request_kind_overrides: HashMap::new(),
                agent_handoff_preparations_pending: HashMap::new(),
                agent_handoff_sends_pending: HashMap::new(),
                agent_chat_agent_selection: HashMap::new(),
                agent_chat_agent_dismissed_query: HashMap::new(),
                agent_chat_project_selection: HashMap::new(),
                agent_chat_project_dismissed_query: HashMap::new(),
                agent_chat_expanded_thoughts: HashSet::new(),
                agent_chat_expanded_work_log_groups: HashSet::new(),
                agent_chat_expanded_work_log_entries: HashSet::new(),
                agent_chat_footer_tasks_expanded: HashSet::new(),
                agent_chat_queue_expanded: HashSet::new(),
                agent_chat_usage_expanded: HashSet::new(),
                agent_chat_render_sessions: HashMap::new(),
                rendered_chat_agents: HashSet::new(),
                native_surface_dirty: true,
                native_overlay_open: false,
                agent_chat_transcript_views: HashMap::new(),
                agent_status_seen: HashMap::new(),
                agent_verify_scan_seen: HashMap::new(),
                verification_prompt_pending: HashSet::new(),
                agent_chat_list_states: HashMap::new(),
                agent_chat_row_fingerprints: HashMap::new(),
                agent_chat_list_top_down: HashMap::new(),
                agent_chat_hydrating: HashSet::new(),
                agent_chat_hydration_generations: HashMap::new(),
                agent_chat_post_hydration_submissions: HashMap::new(),
                agent_chat_history: HashMap::new(),
                agent_chat_prepended_rows: HashMap::new(),
                agent_chat_scrolled_up: HashMap::new(),
                agent_chat_search: None,
                agent_chat_reveal: HashMap::new(),
                agent_chat_reveal_pending: HashSet::new(),
                agent_chat_active_reveal: None,
                agent_chat_hovered_message: None,
                agent_chat_expanded_user_messages: HashSet::new(),
                agent_auto_names_requested: HashSet::new(),
                active_chat_visualization: None,
                auto_loaded_chat_visualizations: HashSet::new(),
                agent_chat_visualization_sync_stamps: HashMap::new(),
                agent_diff_drawers: HashMap::new(),
                agent_ship_pr_targets: HashMap::new(),
                agent_ship_prs: HashMap::new(),
                agent_ship_pr_checked_at: HashMap::new(),
                agent_ship_pr_fetching: HashSet::new(),
                ship_task_ui: HashMap::new(),
                agent_detail_tabs: HashMap::new(),
                agent_notes_expanded: HashSet::new(),
                personal_editor: None,
                personal_editor_save_epoch: 0,
                task_desc_expanded: HashSet::new(),
                task_status_updating: HashSet::new(),
                task_status_error: HashMap::new(),
                task_comment_inputs: HashMap::new(),
                task_comment_posting: HashSet::new(),
                task_comment_error: HashMap::new(),
                services_reveal: false,
                services_env_edit: None,
                hovered_agent_title: None,
                hovered_task_title: false,
                task_title_edit: None,
                hovered_doc_title: false,
                agent_title_edit: None,
                agent_start_errors: HashMap::new(),
                lane_setups: HashMap::new(),
                solo_docs_refresh_generation: 0,
                lane_exit_pending: HashSet::new(),
                rejoin_ready_checks: HashMap::new(),
                rejoin_ready_inflight: HashSet::new(),
                lane_preview_pending: HashSet::new(),
                project_preview_refresh: HashMap::new(),
                memory_card_ids_seen: HashSet::new(),
                brain_poll_bootstrapped: false,
                memory_undos_pending: HashSet::new(),
                orbit_undos_pending: HashSet::new(),
                agent_summaries: HashMap::new(),
                agent_summary_requests_pending: HashMap::new(),
                agent_summary_silent_requests: HashSet::new(),
                agent_summary_maintenance_status_seen: HashMap::new(),
                pocketcomet_handoffs_pending: HashSet::new(),
                agent_record_status_seen,
                agent_messages_inflight: HashSet::new(),
                agent_message_cards_expanded: HashSet::new(),
                memory_distills_inflight: HashSet::new(),
                memory_proposal_accepts_pending: HashSet::new(),
                memory_proposal_errors: HashMap::new(),
                remote_command_ids: HashSet::new(),
                remote_ship_status: HashMap::new(),
                voice_composer_pending: VecDeque::new(),
                selected_file: HashMap::new(),
                pending_editor_positions: HashMap::new(),
                pending_reference_open: None,
                selected_db: HashMap::new(),
                doc_title_edit: None,
                doc_action_error: None,
                doc_label_inputs: HashMap::new(),
                docs_focus_mode: DocsFocusMode::Doc,
                context_mode: ContextMode::Docs,
                docs_terminal_mode: DocsTerminalMode::Assistant,
                doc_assistant_inputs: HashMap::new(),
                doc_assistant_errors: HashMap::new(),
                doc_assistant_terminal_open: HashMap::new(),
                doc_assistant_pending_messages: HashMap::new(),
                doc_assistant_list_states: HashMap::new(),
                doc_assistant_panel_width: 440.0,
                doc_assistant_resize: None,
                new_agent_composer: None,
                new_agent_drafts: HashMap::new(),
                onboarding_composer_nudged: false,
                composer_file_cache: HashMap::new(),
                composer_file_cache_loading: HashSet::new(),
                composer_branch_query,
                composer_model_query,
                composer_branch_expanded: false,
                composer_model_expanded: false,
                composer_model_provider: None,
                composer_model_favorites_only: false,
                open_code_catalog: OpenCodeCatalog::default(),
                agent_chat_rail_compact: false,
                pocketcomet_project_filter: None,
                pocketcomet_selected_chat: None,
                view_mode: CenterMode::Agents,
                last_code_mode: CenterMode::Split,
                project_navigation,
                view_history_back: Vec::new(),
                view_history_forward: Vec::new(),
                tasks_refresh_epoch: 0,
                tasks_detail_collapsed: false,
                agents_panel_reset_epoch: 0,
                git_diff_open_epoch: 0,
            }
        });
        center.update(cx, |this, cx| this.schedule_solo_docs_refresh(cx));
        center
    }
}

impl CenterArea {
    fn process_native_messages(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let doc_messages = self
            .web_host
            .update(cx, |host, _| host.take_doc_editor_messages());
        for message in doc_messages {
            match message {
                web_preview::DocEditorMessage::Change { path, document } => {
                    if let Err(error) = self
                        .docs
                        .update(cx, |docs, cx| docs.apply_web_document(path, document, cx))
                    {
                        eprintln!("failed to apply document editor change: {error:#}");
                    }
                }
                web_preview::DocEditorMessage::OpenReference { path, target } => {
                    self.pending_reference_open = Some((path, target));
                }
                _ => {}
            }
        }
        self.handle_project_preview_messages(window, cx);
        self.apply_pending_reference_open(window, cx);

        self.process_studio_messages(window, cx);
        let studio_active = self.view_mode == CenterMode::Design
            && self.studio.as_ref().is_some_and(|s| self.workspace.read(cx).active == Some(s.project));
        if !studio_active && self.studio.as_ref().is_some_and(|s| s.screen.is_none() && s.canvas.html.is_some()) { self.flush_studio_canvas(); }
        self.process_studio_canvas(studio_active && !window.has_active_dialog(cx) && !window.has_active_sheet(cx), cx);
        self.refresh_studio_canvas(cx);
    }
}
