use super::*;

fn conversation_files_for_repository(
    workspace_root: &Path,
    repository_root: &Path,
    repository_files: &[PathBuf],
    workspace_files: &std::collections::BTreeSet<PathBuf>,
) -> Vec<PathBuf> {
    let repository_prefix = repository_root
        .strip_prefix(workspace_root)
        .unwrap_or(Path::new(""));
    repository_files
        .iter()
        .filter(|path| workspace_files.contains(&repository_prefix.join(path)))
        .cloned()
        .collect()
}

fn refreshed_conversation_files(
    dirty: &[PathBuf],
    previous: &[PathBuf],
    pending: Option<&HashSet<PathBuf>>,
) -> Vec<PathBuf> {
    dirty
        .iter()
        .filter(|p| pending.map_or_else(|| previous.contains(p), |paths| paths.contains(*p)))
        .cloned()
        .collect()
}

fn refresh_single_ship_inventory(dialog: &mut AgentShipDialog, cx: &mut Context<AgentShipDialog>) {
    if let Some(snapshot) = dialog.git.read(cx).snapshot.as_ref() {
        let dirty = snapshot
            .entries
            .iter()
            .map(|e| e.path.clone())
            .collect::<HashSet<_>>();
        dialog.all_files = dirty.iter().cloned().collect();
        dialog.all_files.sort();
        dialog.staged_files = snapshot
            .entries
            .iter()
            .filter(|e| e.staged.is_some())
            .map(|e| e.path.clone())
            .collect();
        dialog.file_kinds = snapshot
            .entries
            .iter()
            .filter_map(|e| Some((e.path.clone(), e.staged.or(e.unstaged)?)))
            .collect();
        let pending = ide_core::agent_changes::cached_pending_repository_paths(
            dialog.agent_id,
            &dialog.repo_path,
        );
        dialog.conversation_files = refreshed_conversation_files(
            &dialog.all_files,
            &dialog.conversation_files,
            pending.as_ref(),
        );
    }
}
fn refresh_multi_ship_inventory(
    dialog: &mut MultiRepoShipDialog,
    cx: &mut Context<MultiRepoShipDialog>,
) {
    for repository in &mut dialog.repositories {
        if let Some(snapshot) = repository.git.read(cx).snapshot.as_ref() {
            let dirty = snapshot
                .entries
                .iter()
                .map(|e| e.path.clone())
                .collect::<HashSet<_>>();
            repository.all_files = dirty.iter().cloned().collect();
            repository.all_files.sort();
            repository.staged_files = snapshot
                .entries
                .iter()
                .filter(|e| e.staged.is_some())
                .map(|e| e.path.clone())
                .collect();
            repository.file_kinds = snapshot
                .entries
                .iter()
                .filter_map(|e| Some((e.path.clone(), e.staged.or(e.unstaged)?)))
                .collect();
            let pending = ide_core::agent_changes::cached_pending_repository_paths(
                dialog.agent_id,
                &repository.repo_path,
            );
            repository.conversation_files = refreshed_conversation_files(
                &repository.all_files,
                &repository.conversation_files,
                pending.as_ref(),
            );
        }
    }
}

fn append_unique_ship_result(
    timeline: &mut Vec<AgentChatTimelineItem>,
    result: crate::state::agent_chat::ShipResult,
) -> bool {
    if timeline.iter().any(|item| {
        matches!(
            item,
            AgentChatTimelineItem::ShipResult(existing) if existing.id == result.id
        )
    }) {
        return false;
    }
    timeline.push(AgentChatTimelineItem::ShipResult(result));
    true
}

/// Return only the changed receipt for persistence. Shipping must never copy
/// or rewrite the rest of a potentially very large conversation.
fn attach_ship_commit_metadata(
    changed_files: &mut crate::state::agent_chat::ChangedFilesSummary,
    timeline: &mut [AgentChatTimelineItem],
    _ship_snapshot_id: Option<Uuid>,
    commit_sha: &str,
) -> (Option<Uuid>, Option<AgentChatTimelineItem>) {
    let mut snapshot_id = None;
    if !changed_files.is_empty() {
        changed_files.commit_sha = Some(commit_sha.to_owned());
        snapshot_id = changed_files.snapshot_id;
    }
    for item in timeline.iter_mut().rev() {
        let AgentChatTimelineItem::ChangedFiles(summary) = item else {
            continue;
        };
        summary.commit_sha = Some(commit_sha.to_owned());
        snapshot_id = summary.snapshot_id.or(snapshot_id);
        return (
            snapshot_id,
            Some(AgentChatTimelineItem::ChangedFiles(summary.clone())),
        );
    }
    (snapshot_id, None)
}

pub(super) fn exact_agent_ship_paths(
    root: &Path,
    summary: &crate::state::agent_chat::ChangedFilesSummary,
) -> std::collections::BTreeSet<PathBuf> {
    summary
        .files
        .iter()
        .map(|file| normalize_agent_ship_path(root, &file.path))
        .collect()
}

/// Select the branch Ship was opened from and prefer it after pinned branches
/// in the PR base picker. The remote default remains the fallback for detached
/// HEADs and older Solo records that do not remember their fork branch.
fn prefer_ship_base_branch(
    preferred: Option<&str>,
    fallback: &str,
    options: &mut Vec<String>,
) -> String {
    let selected = preferred
        .map(str::trim)
        .filter(|branch| !branch.is_empty())
        .or_else(|| {
            let fallback = fallback.trim();
            (!fallback.is_empty()).then_some(fallback)
        })
        .unwrap_or("main")
        .to_string();

    if let Some(index) = options.iter().position(|branch| branch == &selected) {
        options.remove(index);
    }
    options.insert(0, selected.clone());
    selected
}

impl CenterArea {
    pub(in crate::ui::center) fn attach_ship_commit_to_changed_files(
        &mut self,
        agent_id: Uuid,
        ship_snapshot_id: Option<Uuid>,
        commit_sha: String,
        cx: &mut Context<Self>,
    ) {
        let snapshot_to_update = self.agent_chats.update(cx, |chats, cx| {
            let Some(session) = chats.sessions.get_mut(&agent_id) else {
                return None;
            };
            let (snapshot_id, receipt) = attach_ship_commit_metadata(
                &mut session.changed_files,
                &mut session.timeline,
                ship_snapshot_id,
                &commit_sha,
            );
            if let Some(receipt) = receipt {
                crate::state::agent_chat::persist_timeline_item(agent_id, receipt, cx);
            }
            chats.publish_change(
                agent_id,
                crate::state::agent_chat::ChatChangeCategories::CONTENT,
                cx,
            );
            snapshot_id
        });

        if let Some(snapshot_id) = snapshot_to_update {
            cx.background_executor()
                .spawn(async move {
                    if let Err(error) =
                        ide_core::local_store::LocalStore::open_default().and_then(|store| {
                            store.update_agent_diff_snapshot_commit(snapshot_id, &commit_sha)
                        })
                    {
                        eprintln!("failed to update changed-files snapshot commit SHA: {error:#}");
                    }
                })
                .detach();
        }
    }

    pub(in crate::ui::center) fn append_agent_ship_result(
        &mut self,
        agent_id: Uuid,
        repository: Option<String>,
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

        let ship_id = repository
            .as_ref()
            .map(|repository| format!("{}:{}:{}", repository, outcome.branch, outcome.commit_sha))
            .unwrap_or_else(|| format!("{}:{}", outcome.branch, outcome.commit_sha));
        let ship_result = crate::state::agent_chat::ShipResult {
            id: ship_id.clone(),
            action: outcome.action.clone(),
            repository: repository.clone(),
            branch: outcome.branch.clone(),
            pr_base_branch: outcome.pr_base_branch.clone(),
            commit_sha: outcome.commit_sha.clone(),
            pr_url: outcome.pr_url.clone(),
            pr_title: outcome.pr_title.clone(),
            pr_body: outcome.pr_body.clone(),
            created_at: unix_now_secs(),
            task: task.clone(),
            suggested_status: suggested_status.clone(),
            applied: None,
        };
        self.agent_chats.update(cx, |chats, cx| {
            let Some(session) = chats.sessions.get_mut(&agent_id) else {
                return;
            };
            if append_unique_ship_result(&mut session.timeline, ship_result.clone()) {
                crate::state::agent_chat::persist_timeline_item(
                    agent_id,
                    AgentChatTimelineItem::ShipResult(ship_result),
                    cx,
                );
                chats.publish_change(
                    agent_id,
                    crate::state::agent_chat::ChatChangeCategories::CONTENT,
                    cx,
                );
            }
        });

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
        if agent.repository_path.is_none()
            && !agent.is_active_solo()
            && self.open_multi_repo_ship_dialog(&agent, window, cx)
        {
            return;
        }
        // A materialized Solo ships from its lane: same execution path, the
        // lane's own snapshot as the source instead of the project GitState.
        let solo_lane = agent
            .is_active_solo()
            .then(|| agent.solo_branch.clone().zip(agent.lane_path.clone()))
            .flatten();
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
        let whole_workspace = agent.repository_path.is_none() && !agent.is_active_solo();
        let related_root = if whole_workspace {
            agent.project_path.as_path()
        } else {
            agent.runtime_path()
        };
        if let Some(session) = self.agent_chats.read(cx).session(agent.id) {
            related = exact_agent_ship_paths(related_root, &session.changed_files);
        } else {
            // Legacy terminal agents have no chat ledger. Once a chat session
            // exists, its attributed projection is authoritative and the old
            // agent cache must not reintroduce another chat's dirty paths.
            for file in &agent.changed_files {
                related.insert(normalize_agent_ship_path(related_root, &file.path));
            }
        }

        if let Some(pending) =
            ide_core::agent_changes::cached_pending_paths(agent.id, agent.runtime_path())
        {
            related.retain(|path| pending.contains(path));
        }
        let conversation_files = if whole_workspace {
            conversation_files_for_repository(&agent.project_path, &repo_path, &all_files, &related)
        } else {
            all_files
                .iter()
                .filter(|path| related.contains(*path))
                .cloned()
                .collect::<Vec<_>>()
        };
        let branch_name = cx.new(|cx| InputState::new(window, cx).placeholder("Generated branch"));
        let commit_message = cx.new(|cx| {
            InputState::new(window, cx)
                .auto_grow(3, 8)
                .placeholder("Generate or enter manually")
        });
        let default_pr_base_branch = crate::ui::git::git_panel::default_remote_branch(&repo_path);
        let mut pr_base_branch_options = {
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
        let preferred_pr_base_branch = if solo_lane.is_some() {
            agent.solo_base_branch.as_deref()
        } else {
            branch.as_deref()
        };
        let pr_base_branch = prefer_ship_base_branch(
            preferred_pr_base_branch,
            &default_pr_base_branch,
            &mut pr_base_branch_options,
        );
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
            .map(|(_, lane)| (agent.repository_root().to_path_buf(), lane.clone()));
        let all_changes = crate::ui::onboarding::ship_defaults_to_all_changes(agent.project_id, cx);
        let onboarding_demo =
            crate::ui::onboarding::ship_uses_demo_pull_request(agent.project_id, cx);
        let center = cx.entity().downgrade();
        let dialog = cx.new(|cx| {
            let initial_revision = ide_core::agent_changes::pending_revision();
            cx.spawn(async move |this, cx| {
                let mut revision = initial_revision;
                loop {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(250))
                        .await;
                    if this.upgrade().is_none() {
                        break;
                    }
                    let next = ide_core::agent_changes::pending_revision();
                    if next != revision {
                        revision = next;
                        if this
                            .update(cx, |dialog, cx| {
                                refresh_single_ship_inventory(dialog, cx);
                                cx.notify();
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            })
            .detach();
            cx.observe(&git, |dialog: &mut AgentShipDialog, _, cx| {
                refresh_single_ship_inventory(dialog, cx);
                cx.notify();
            })
            .detach();
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
                pending_commit: None,
                summary_maintenance_started: false,
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

    fn open_multi_repo_ship_dialog(
        &mut self,
        agent: &AgentRecord,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let mut related = std::collections::BTreeSet::new();
        if let Some(session) = self.agent_chats.read(cx).session(agent.id) {
            related = exact_agent_ship_paths(&agent.project_path, &session.changed_files);
        } else {
            for file in &agent.changed_files {
                related.insert(normalize_agent_ship_path(&agent.project_path, &file.path));
            }
        }

        if let Some(pending) =
            ide_core::agent_changes::cached_pending_paths(agent.id, agent.runtime_path())
        {
            related.retain(|path| pending.contains(path));
        }
        let git_repositories = self.git_states.read(cx).repositories(agent.project_id);
        let mut repositories = Vec::new();
        for git in git_repositories {
            let state = git.read(cx);
            let Some(snapshot) = state.snapshot.as_ref() else {
                continue;
            };
            let mut all_files = snapshot
                .entries
                .iter()
                .filter(|entry| entry.staged.is_some() || entry.unstaged.is_some())
                .map(|entry| entry.path.clone())
                .collect::<Vec<_>>();
            all_files.sort();
            all_files.dedup();
            if all_files.is_empty() {
                continue;
            }
            let repo_path = state.repo_path.clone();
            let conversation_files = conversation_files_for_repository(
                &agent.project_path,
                &repo_path,
                &all_files,
                &related,
            );
            let mut staged_files = snapshot
                .entries
                .iter()
                .filter(|entry| entry.staged.is_some())
                .map(|entry| entry.path.clone())
                .collect::<Vec<_>>();
            staged_files.sort();
            staged_files.dedup();
            let file_kinds = snapshot
                .entries
                .iter()
                .filter_map(|entry| Some((entry.path.clone(), entry.staged.or(entry.unstaged)?)))
                .collect::<HashMap<_, _>>();
            let branch = snapshot.head.branch.clone();
            let needs_upstream = snapshot
                .branches
                .iter()
                .find(|branch| branch.is_head)
                .map(|branch| branch.upstream.is_none())
                .unwrap_or(false);
            let default_base = crate::ui::git::git_panel::default_remote_branch(&repo_path);
            let mut pr_base_branch_options =
                crate::ui::git::git_panel::pull_request_base_branch_options(
                    &default_base,
                    &snapshot.branches,
                );
            let pr_base_branch = prefer_ship_base_branch(
                branch.as_deref(),
                &default_base,
                &mut pr_base_branch_options,
            );
            let label = repo_path
                .strip_prefix(&agent.project_path)
                .ok()
                .filter(|relative| !relative.as_os_str().is_empty())
                .map(|relative| relative.to_string_lossy().into_owned())
                .unwrap_or_else(|| {
                    repo_path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| repo_path.display().to_string())
                });
            repositories.push(MultiRepoShipRepository {
                label,
                git,
                tracked_repo_path: repo_path.clone(),
                repo_path,
                branch,
                needs_upstream,
                all_files,
                conversation_files,
                staged_files,
                file_kinds,
                deselected: HashSet::new(),
                branch_name: cx
                    .new(|cx| InputState::new(window, cx).placeholder("Generated branch")),
                commit_message: cx.new(|cx| {
                    InputState::new(window, cx)
                        .auto_grow(3, 8)
                        .placeholder("Generate or enter manually")
                }),
                pr_base_branch,
                pr_base_branch_options,
                pr_title: cx.new(|cx| {
                    InputState::new(window, cx).placeholder("Generate or enter manually")
                }),
                pr_description: cx.new(|cx| {
                    InputState::new(window, cx)
                        .auto_grow(4, 9)
                        .placeholder("Generate or enter manually")
                }),
                pending_commit: None,
                completed: false,
            });
        }
        if repositories.len() < 2 {
            return false;
        }

        let all_changes = crate::ui::onboarding::ship_defaults_to_all_changes(agent.project_id, cx);
        let create_branch =
            crate::ui::onboarding::ship_defaults_to_new_branch(agent.project_id, cx);
        let center = cx.entity().downgrade();
        let dialog = cx.new(|cx| {
            let initial_revision = ide_core::agent_changes::pending_revision();
            cx.spawn(async move |this, cx| {
                let mut revision = initial_revision;
                loop {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(250))
                        .await;
                    if this.upgrade().is_none() {
                        break;
                    }
                    let next = ide_core::agent_changes::pending_revision();
                    if next != revision {
                        revision = next;
                        if this
                            .update(cx, |dialog, cx| {
                                refresh_multi_ship_inventory(dialog, cx);
                                cx.notify();
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            })
            .detach();
            for repository in &repositories {
                cx.observe(
                    &repository.git,
                    |dialog: &mut MultiRepoShipDialog, _, cx| {
                        refresh_multi_ship_inventory(dialog, cx);
                        cx.notify();
                    },
                )
                .detach();
            }
            MultiRepoShipDialog {
                agent_id: agent.id,
                project_id: agent.project_id,
                center,
                agent_title: agent.title.clone(),
                generation_agent: self.workspace.read(cx).generation_agent.clone(),
                repositories,
                selected_repository: 0,
                files_collapsed: false,
                create_branch,
                scope: if all_changes {
                    AgentShipScope::All
                } else {
                    AgentShipScope::Conversation
                },
                push: true,
                open_pr: false,
                auto_ship: false,
                busy: false,
                prepared: false,
                summary_maintenance_started: false,
                error: None,
            }
        });
        window.open_dialog(cx, move |dialog_view, _, _| {
            dialog_view
                .title("Ship agent work")
                // Preserve the existing Ship form's content width; the extra
                // space belongs only to the new repository/settings rail.
                .w(px(960.))
                .margin_top(px(20.))
                .overlay_closable(false)
                .child(dialog.clone())
        });
        crate::ui::onboarding::emit_for_project(
            agent.project_id,
            crate::ui::onboarding::OnboardingEvent::ShipOpened,
            cx,
        );
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_paths_and_new_paths_return_for_later_confirmed_contributions() {
        let dirty = vec![
            PathBuf::from("shared.rs"),
            PathBuf::from("new.rs"),
            PathBuf::from("manual.rs"),
        ];
        let settled = HashSet::new();
        assert!(
            refreshed_conversation_files(&dirty, &["shared.rs".into()], Some(&settled)).is_empty()
        );
        let later = HashSet::from([PathBuf::from("shared.rs"), PathBuf::from("new.rs")]);
        assert_eq!(
            refreshed_conversation_files(&dirty, &[], Some(&later)),
            vec![PathBuf::from("shared.rs"), PathBuf::from("new.rs")]
        );
    }

    #[test]
    fn ship_metadata_updates_only_the_latest_receipt_in_a_large_chat() {
        use crate::state::agent_chat::{ChangedFilesSummary, FileChangeStat};

        let previous = ChangedFilesSummary {
            commit_sha: Some("previous-commit".into()),
            ..ChangedFilesSummary::attributed(
                "previous-turn",
                vec![FileChangeStat::new("previous.rs", 1, 0)],
                Vec::new(),
            )
        };
        let mut timeline = vec![AgentChatTimelineItem::ChangedFiles(previous.clone()); 2_000];
        let mut ledger = ChangedFilesSummary::attributed(
            "current-turn",
            (0..250)
                .map(|i| FileChangeStat::new(format!("file-{i}.rs"), 2, 1))
                .collect(),
            Vec::new(),
        );
        timeline.push(AgentChatTimelineItem::ChangedFiles(ledger.clone()));
        let snapshot_id = Uuid::new_v4();

        let (snapshot, receipt) = attach_ship_commit_metadata(
            &mut ledger,
            &mut timeline,
            Some(snapshot_id),
            "shipped-commit",
        );

        assert_eq!(snapshot, None);
        assert_eq!(ledger.commit_sha.as_deref(), Some("shipped-commit"));
        assert_eq!(timeline.len(), 2_001);
        for item in &timeline[..2_000] {
            assert!(
                matches!(item, AgentChatTimelineItem::ChangedFiles(summary) if summary == &previous)
            );
        }
        let Some(AgentChatTimelineItem::ChangedFiles(receipt)) = receipt else {
            panic!("the latest receipt must be returned for incremental persistence");
        };
        assert_eq!(receipt.turn_id.as_deref(), Some("current-turn"));
        assert_eq!(receipt.files.len(), 250);
        assert_eq!(receipt.snapshot_id, None);
        assert_eq!(receipt.commit_sha.as_deref(), Some("shipped-commit"));
    }

    #[test]
    fn ship_metadata_preserves_existing_snapshots_and_handles_missing_receipts() {
        use crate::state::agent_chat::{ChangedFilesSummary, FileChangeStat};

        let ledger_snapshot = Uuid::new_v4();
        let receipt_snapshot = Uuid::new_v4();
        let mut ledger = ChangedFilesSummary {
            snapshot_id: Some(ledger_snapshot),
            ..ChangedFilesSummary::attributed(
                "turn",
                vec![FileChangeStat::new("file.rs", 1, 0)],
                Vec::new(),
            )
        };
        let mut receipt = ledger.clone();
        receipt.snapshot_id = Some(receipt_snapshot);
        let mut timeline = vec![AgentChatTimelineItem::ChangedFiles(receipt)];
        let (snapshot, _) =
            attach_ship_commit_metadata(&mut ledger, &mut timeline, Some(Uuid::new_v4()), "commit");
        assert_eq!(snapshot, Some(receipt_snapshot));
        assert_eq!(ledger.snapshot_id, Some(ledger_snapshot));

        let (snapshot, receipt) =
            attach_ship_commit_metadata(&mut ledger, &mut [], Some(Uuid::new_v4()), "next-commit");
        assert_eq!(snapshot, Some(ledger_snapshot));
        assert!(receipt.is_none());
        assert_eq!(ledger.commit_sha.as_deref(), Some("next-commit"));
    }

    #[test]
    fn maps_workspace_changed_files_into_nested_repository_scope() {
        let workspace = Path::new("/workspace");
        let repository = workspace.join("apps/frontend");
        let repository_files = vec![
            PathBuf::from("src/app.rs"),
            PathBuf::from("src/unrelated.rs"),
        ];
        let workspace_files = [
            PathBuf::from("apps/frontend/src/app.rs"),
            PathBuf::from("services/backend/src/api.rs"),
        ]
        .into_iter()
        .collect();

        assert_eq!(
            conversation_files_for_repository(
                workspace,
                &repository,
                &repository_files,
                &workspace_files,
            ),
            vec![PathBuf::from("src/app.rs")],
        );
    }

    #[test]
    fn this_chat_shipping_excludes_command_observations() {
        let summary = crate::state::agent_chat::ChangedFilesSummary::attributed(
            "turn-1",
            vec![crate::state::agent_chat::FileChangeStat::new(
                "src/exact.rs",
                1,
                0,
            )],
            vec![crate::state::agent_chat::FileChangeStat::new(
                "src/concurrent.rs",
                4,
                0,
            )],
        );

        assert_eq!(
            exact_agent_ship_paths(Path::new("/workspace"), &summary),
            [PathBuf::from("src/exact.rs")].into_iter().collect()
        );
    }

    #[test]
    fn ship_base_prefers_the_branch_active_when_ship_opened() {
        let mut options = vec!["main".to_string(), "dev".to_string()];

        let selected = prefer_ship_base_branch(Some("dev"), "main", &mut options);

        assert_eq!(selected, "dev");
        assert_eq!(options, vec!["dev", "main"]);
    }

    #[test]
    fn ship_base_keeps_remote_default_as_the_fallback() {
        let mut options = vec!["main".to_string(), "dev".to_string()];

        let selected = prefer_ship_base_branch(None, "main", &mut options);

        assert_eq!(selected, "main");
        assert_eq!(options, vec!["main", "dev"]);
    }

    #[test]
    fn ship_result_is_appended_once() {
        let mut timeline = Vec::new();
        let result = crate::state::agent_chat::ShipResult {
            id: "feature:abc123".to_string(),
            action: "Commit + push + PR".to_string(),
            repository: None,
            branch: "feature".to_string(),
            pr_base_branch: Some("dev".to_string()),
            commit_sha: "abc123".to_string(),
            pr_url: Some("https://github.com/acme/app/pull/1".to_string()),
            pr_title: Some("Ship the fix".to_string()),
            pr_body: None,
            created_at: 2,
            task: None,
            suggested_status: None,
            applied: None,
        };

        assert!(append_unique_ship_result(&mut timeline, result.clone()));
        assert!(matches!(
            timeline.last(),
            Some(AgentChatTimelineItem::ShipResult(appended)) if appended.id == result.id
        ));
        assert!(!append_unique_ship_result(&mut timeline, result));
    }
}
