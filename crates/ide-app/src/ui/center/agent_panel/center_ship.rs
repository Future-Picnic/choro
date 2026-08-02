use super::*;

impl CenterArea {
    pub(in crate::ui::center) fn attach_ship_commit_to_changed_files(
        &mut self,
        agent_id: Uuid,
        ship_snapshot_id: Option<Uuid>,
        commit_sha: String,
        cx: &mut Context<Self>,
    ) {
        let mut timeline_to_persist = None;
        let mut snapshot_to_update = None;
        self.agent_chats.update(cx, |chats, cx| {
            let Some(session) = chats.sessions.get_mut(&agent_id) else {
                return;
            };
            if !session.changed_files.files.is_empty() {
                if session.changed_files.snapshot_id.is_none() {
                    session.changed_files.snapshot_id = ship_snapshot_id;
                }
                session.changed_files.commit_sha = Some(commit_sha.clone());
                snapshot_to_update = session.changed_files.snapshot_id;
            }
            for item in session.timeline.iter_mut().rev() {
                let AgentChatTimelineItem::ChangedFiles(summary) = item else {
                    continue;
                };
                if summary.snapshot_id.is_none() {
                    summary.snapshot_id = ship_snapshot_id;
                }
                summary.commit_sha = Some(commit_sha.clone());
                snapshot_to_update = summary.snapshot_id.or(snapshot_to_update);
                break;
            }
            timeline_to_persist = Some(session.timeline.clone());
            cx.notify();
        });

        if let Some(snapshot_id) = snapshot_to_update {
            if let Ok(store) = ide_core::local_store::LocalStore::open_default() {
                if let Err(error) =
                    store.update_agent_diff_snapshot_commit(snapshot_id, &commit_sha)
                {
                    eprintln!("failed to update changed-files snapshot commit SHA: {error:#}");
                }
            }
        }
        if let Some(timeline) = timeline_to_persist {
            if let Err(error) = persist_timeline_snapshot(agent_id, &timeline) {
                eprintln!("failed to persist changed-files ship metadata: {error:#}");
            }
        }
    }

    pub(in crate::ui::center) fn append_agent_ship_result(
        &mut self,
        agent_id: Uuid,
        outcome: &AgentShipOutcome,
        cx: &mut Context<Self>,
    ) {
        // Resolve the task this ship relates to (source task, else the first
        // linked task) and the status suggestion from its connection settings.
        let (task, suggested_status) = self
            .agents
            .read(cx)
            .agent(agent_id)
            .and_then(|agent| {
                let task = agent
                    .source_task
                    .clone()
                    .or_else(|| agent.linked_tasks.first().cloned())?;
                Some((task, agent.project_id))
            })
            .map(|(task, project)| {
                let suggested = self
                    .tasks
                    .read(cx)
                    .connection_object_for_ref(project, &task, cx)
                    .and_then(|connection| connection.pr_done_status());
                (Some(task), suggested)
            })
            .unwrap_or((None, None));

        let ship_id = format!("{}:{}", outcome.branch, outcome.commit_sha);
        let mut timeline_to_persist = None;
        self.agent_chats.update(cx, |chats, cx| {
            let Some(session) = chats.sessions.get_mut(&agent_id) else {
                return;
            };
            let id = ship_id.clone();
            if session.timeline.iter().any(|item| {
                matches!(
                    item,
                    AgentChatTimelineItem::ShipResult(result) if result.id == id
                )
            }) {
                return;
            }
            session.timeline.push(AgentChatTimelineItem::ShipResult(
                crate::state::agent_chat::ShipResult {
                    id,
                    action: outcome.action.clone(),
                    branch: outcome.branch.clone(),
                    commit_sha: outcome.commit_sha.clone(),
                    pr_url: outcome.pr_url.clone(),
                    pr_title: outcome.pr_title.clone(),
                    pr_body: outcome.pr_body.clone(),
                    created_at: unix_now_secs(),
                    task: task.clone(),
                    suggested_status: suggested_status.clone(),
                    applied: None,
                },
            ));
            timeline_to_persist = Some(session.timeline.clone());
            cx.notify();
        });

        if let Some(timeline) = timeline_to_persist {
            if let Err(error) = persist_timeline_snapshot(agent_id, &timeline) {
                eprintln!("failed to persist agent ship result: {error:#}");
            }
        }

        // Pre-seed the post-ship "update the task" card and warm up its status
        // list so the controls are ready the moment the card appears.
        if let (Some(task), Some(pr_url)) = (task, outcome.pr_url.clone()) {
            if let Some(project) = self.agents.read(cx).agent(agent_id).map(|a| a.project_id) {
                let seed = ShipTaskSeed {
                    id: ship_id,
                    agent_id,
                    project,
                    task,
                    comment_body: ship_task_comment_body(&outcome.pr_title, &pr_url),
                    suggested_status,
                };
                self.seed_ship_task_ui(&seed, cx);
            }
        }
    }

    pub(in crate::ui::center) fn track_agent_ship_pr_branch(
        &mut self,
        agent_id: Uuid,
        repo_path: PathBuf,
        branch: String,
        cx: &mut Context<Self>,
    ) {
        let target = (repo_path.clone(), branch.clone());
        if self.agent_ship_pr_targets.get(&agent_id) != Some(&target) {
            self.agent_ship_pr_targets.insert(agent_id, target);
            self.agent_ship_prs.remove(&agent_id);
            self.agent_ship_pr_checked_at.remove(&agent_id);
            self.agent_ship_pr_fetching.remove(&agent_id);
        }
        self.agents.update(cx, |agents, cx| {
            agents.update_ship_pr_branch(agent_id, repo_path, branch, cx);
        });
        cx.notify();
    }

    pub(in crate::ui::center) fn sync_agent_ship_pull_request(
        &mut self,
        agent_id: Uuid,
        cx: &mut Context<Self>,
    ) {
        let Some((repo_path, branch)) = self.agent_ship_pr_targets.get(&agent_id).cloned() else {
            return;
        };
        let refresh_interval = if self.agent_ship_prs.contains_key(&agent_id) {
            AGENT_SHIP_PR_REFRESH_INTERVAL
        } else {
            AGENT_SHIP_PR_MISSING_REFRESH_INTERVAL
        };
        let stale = self
            .agent_ship_pr_checked_at
            .get(&agent_id)
            .is_none_or(|checked_at| checked_at.elapsed() > refresh_interval);
        if !stale || self.agent_ship_pr_fetching.contains(&agent_id) {
            return;
        }

        self.agent_ship_pr_checked_at
            .insert(agent_id, Instant::now());
        self.agent_ship_pr_fetching.insert(agent_id);
        cx.spawn(async move |this, cx| {
            let pr =
                cx.background_executor()
                    .spawn({
                        let repo_path = repo_path.clone();
                        let branch = branch.clone();
                        async move {
                            crate::ui::git::git_panel::branch_pull_request(&repo_path, &branch)
                        }
                    })
                    .await;

            this.update(cx, |center, cx| {
                center.agent_ship_pr_fetching.remove(&agent_id);
                if center.agent_ship_pr_targets.get(&agent_id)
                    != Some(&(repo_path.clone(), branch.clone()))
                {
                    return;
                }
                if let Some(pr) = pr {
                    center.agent_ship_prs.insert(agent_id, pr);
                } else {
                    center.agent_ship_prs.remove(&agent_id);
                    let retry_repo = repo_path.clone();
                    let retry_branch = branch.clone();
                    cx.spawn(async move |this, cx| {
                        cx.background_executor()
                            .timer(AGENT_SHIP_PR_MISSING_REFRESH_INTERVAL)
                            .await;
                        this.update(cx, |center, cx| {
                            if center.agent_ship_pr_targets.get(&agent_id)
                                == Some(&(retry_repo, retry_branch))
                                && !center.agent_ship_prs.contains_key(&agent_id)
                                && !center.agent_ship_pr_fetching.contains(&agent_id)
                            {
                                center.agent_ship_pr_checked_at.remove(&agent_id);
                                cx.notify();
                            }
                        })
                        .ok();
                    })
                    .detach();
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(in crate::ui::center) fn render_agent_ship_pr_indicator(
        &self,
        pr: &crate::ui::git::git_panel::BranchPullRequest,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let (accent, _foreground) = agent_ship_pr_status_colors(pr, cx);
        let url = pr.url.clone();
        crate::ui::design::indicator::subline_link_with_icon(
            ("agent-ship-pr-indicator", pr.number as usize),
            gpui_component::Icon::empty()
                .path("icons/branch.svg")
                .size(crate::ui::design::icon_ind())
                .text_color(accent),
            SharedString::from(format!("#{}", pr.number)),
            cx,
        )
        .when(!url.is_empty(), |indicator| {
            indicator.on_click(move |_, _, _| crate::ui::git::git_panel::open_url(&url))
        })
        .into_any_element()
    }

    pub(in crate::ui::center) fn open_agent_ship_dialog(
        &mut self,
        agent: AgentRecord,
        git: Entity<GitState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A materialized Solo ships from its lane: same execution path, the
        // lane's own snapshot as the source instead of the project GitState.
        let solo_lane = agent.solo_branch.clone().zip(agent.lane_path.clone());
        let (repo_path, branch, needs_upstream, all_files, staged_files, file_kinds) =
            if let Some((solo_branch, lane)) = solo_lane.clone() {
                let snapshot = ide_core::git::read_snapshot(&lane).ok();
                let needs_upstream = snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.branches.iter().find(|branch| branch.is_head))
                    .map(|branch| branch.upstream.is_none())
                    .unwrap_or(true);
                let mut all_files = snapshot
                    .as_ref()
                    .map(|snapshot| {
                        snapshot
                            .entries
                            .iter()
                            .filter(|entry| entry.staged.is_some() || entry.unstaged.is_some())
                            .map(|entry| entry.path.clone())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                all_files.sort();
                all_files.dedup();
                let mut staged_files = snapshot
                    .as_ref()
                    .map(|snapshot| {
                        snapshot
                            .entries
                            .iter()
                            .filter(|entry| entry.staged.is_some())
                            .map(|entry| entry.path.clone())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                staged_files.sort();
                staged_files.dedup();
                let file_kinds = snapshot
                    .as_ref()
                    .map(|snapshot| {
                        snapshot
                            .entries
                            .iter()
                            .filter_map(|entry| {
                                Some((entry.path.clone(), entry.staged.or(entry.unstaged)?))
                            })
                            .collect::<std::collections::HashMap<_, _>>()
                    })
                    .unwrap_or_default();
                (
                    lane,
                    Some(solo_branch),
                    needs_upstream,
                    all_files,
                    staged_files,
                    file_kinds,
                )
            } else {
                let state = git.read(cx);
                let branch = state
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.head.branch.clone());
                let needs_upstream = state
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.branches.iter().find(|branch| branch.is_head))
                    .map(|branch| branch.upstream.is_none())
                    .unwrap_or(false);
                let mut all_files = state
                    .snapshot
                    .as_ref()
                    .map(|snapshot| {
                        snapshot
                            .entries
                            .iter()
                            .filter(|entry| entry.staged.is_some() || entry.unstaged.is_some())
                            .map(|entry| entry.path.clone())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                all_files.sort();
                all_files.dedup();
                let mut staged_files = state
                    .snapshot
                    .as_ref()
                    .map(|snapshot| {
                        snapshot
                            .entries
                            .iter()
                            .filter(|entry| entry.staged.is_some())
                            .map(|entry| entry.path.clone())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                staged_files.sort();
                staged_files.dedup();
                let file_kinds = state
                    .snapshot
                    .as_ref()
                    .map(|snapshot| {
                        snapshot
                            .entries
                            .iter()
                            .filter_map(|entry| {
                                Some((entry.path.clone(), entry.staged.or(entry.unstaged)?))
                            })
                            .collect::<std::collections::HashMap<_, _>>()
                    })
                    .unwrap_or_default();
                (
                    state.repo_path.clone(),
                    branch,
                    needs_upstream,
                    all_files,
                    staged_files,
                    file_kinds,
                )
            };

        let mut related = std::collections::BTreeSet::new();
        for file in &agent.changed_files {
            related.insert(normalize_agent_ship_path(agent.runtime_path(), &file.path));
        }
        if let Some(session) = self.agent_chats.read(cx).session(agent.id) {
            for file in &session.changed_files.files {
                related.insert(normalize_agent_ship_path(agent.runtime_path(), &file.path));
            }
        }

        let conversation_files = all_files
            .iter()
            .filter(|path| related.contains(*path))
            .cloned()
            .collect::<Vec<_>>();
        let branch_name = cx.new(|cx| InputState::new(window, cx).placeholder("Generated branch"));
        let commit_message = cx.new(|cx| {
            InputState::new(window, cx)
                .auto_grow(3, 8)
                .placeholder("Generate or enter manually")
        });
        let default_pr_base_branch = crate::ui::git::git_panel::default_remote_branch(&repo_path);
        let pr_base_branch_options = {
            let state = git.read(cx);
            state
                .snapshot
                .as_ref()
                .map(|snapshot| {
                    crate::ui::git::git_panel::pull_request_base_branch_options(
                        &default_pr_base_branch,
                        &snapshot.branches,
                    )
                })
                .unwrap_or_else(|| vec![default_pr_base_branch.clone()])
        };
        let pr_base_branch = pr_base_branch_options
            .first()
            .cloned()
            .unwrap_or(default_pr_base_branch);
        let pr_base_branch_query =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search branch…"));
        let pr_title =
            cx.new(|cx| InputState::new(window, cx).placeholder("Generate or enter manually"));
        let pr_description = cx.new(|cx| {
            InputState::new(window, cx)
                .auto_grow(4, 9)
                .placeholder("Generate or enter manually")
        });
        // A Solo is already on its own branch — never default to a new one.
        let create_branch =
            crate::ui::onboarding::ship_defaults_to_new_branch(agent.project_id, cx)
                && solo_lane.is_none();
        let solo_dialog_lane = solo_lane
            .as_ref()
            .map(|(_, lane)| (agent.project_path.clone(), lane.clone()));
        let all_changes = crate::ui::onboarding::ship_defaults_to_all_changes(agent.project_id, cx);
        let onboarding_demo =
            crate::ui::onboarding::ship_uses_demo_pull_request(agent.project_id, cx);
        let center = cx.entity().downgrade();
        let dialog = cx.new(|cx| {
            // Re-render as the user types in the base-branch search field so the
            // filtered list updates live (the dialog owns the input entity).
            cx.subscribe(&pr_base_branch_query, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            })
            .detach();
            AgentShipDialog {
                agent_id: agent.id,
                project_id: agent.project_id,
                center,
                git,
                agent_title: agent.title,
                repo_path,
                generation_agent: self.workspace.read(cx).generation_agent.clone(),
                branch,
                needs_upstream,
                create_branch,
                all_files,
                conversation_files,
                staged_files,
                branch_name,
                commit_message,
                pr_base_branch,
                pr_base_branch_options,
                pr_base_branch_query,
                pr_base_branch_expanded: false,
                pr_title,
                pr_description,
                scope: if all_changes {
                    AgentShipScope::All
                } else {
                    AgentShipScope::Conversation
                },
                push: true,
                open_pr: onboarding_demo,
                onboarding_demo,
                auto_ship: false,
                file_kinds,
                deselected: std::collections::HashSet::new(),
                files_collapsed: false,
                busy: false,
                prepared: false,
                error: None,
                status: None,
                solo_lane: solo_dialog_lane,
            }
        });

        window.open_dialog(cx, move |dialog_view, _, _| {
            dialog_view
                .title("Ship agent work")
                .w(px(680.))
                .margin_top(px(20.))
                .overlay_closable(false)
                .child(dialog.clone())
        });
        crate::ui::onboarding::emit_for_project(
            agent.project_id,
            crate::ui::onboarding::OnboardingEvent::ShipOpened,
            cx,
        );
    }
}
