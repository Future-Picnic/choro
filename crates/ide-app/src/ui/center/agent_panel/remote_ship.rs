//! Remote (phone-initiated) Ship: the desktop ship pipeline without the
//! dialog. The phone sends one request with scope + toggles; commit message,
//! branch name, and PR content are generated on the Mac exactly like the
//! dialog's auto path. Execution is async — the request is acknowledged
//! immediately, progress lives in `CenterArea::remote_ship_status`, and success
//! lands as the same ShipResult timeline card the desktop produces.

use super::*;

use crate::remote::dto::{ShipPreviewDto, ShipRequest, ShipScopeDto};
use crate::remote::{RemoteError, RemoteResult};

/// One remote ship per agent: in flight, or the last failure until the next
/// attempt replaces it. Success needs no entry — the ShipResult card is the
/// durable signal.
#[derive(Clone, Debug)]
pub(in crate::ui::center) enum RemoteShipStatus {
    Shipping { started_at: u64 },
    Failed { message: String, at: u64 },
}

/// Everything a ship needs to know about the agent's working tree, read
/// straight from the repository. Using the runtime path keeps one code path
/// for Solos (their lane) and shared-tree agents (the project root), and works
/// even when the project isn't focused on the desktop.
struct RemoteShipSource {
    repo: PathBuf,
    branch: Option<String>,
    needs_upstream: bool,
    all_files: Vec<PathBuf>,
    staged_files: Vec<PathBuf>,
    conversation_files: Vec<PathBuf>,
    solo: bool,
}

impl CenterArea {
    fn remote_ship_source(&self, agent: &AgentRecord, cx: &App) -> RemoteResult<RemoteShipSource> {
        let repo = agent.runtime_path().to_path_buf();
        let snapshot = ide_core::git::read_snapshot(&repo).map_err(|error| {
            RemoteError::conflict(format!("could not read git state: {error:#}"))
        })?;
        let branch = snapshot.head.branch.clone();
        let needs_upstream = snapshot
            .branches
            .iter()
            .find(|branch| branch.is_head)
            .map(|branch| branch.upstream.is_none())
            .unwrap_or(true);

        let mut all_files = snapshot
            .entries
            .iter()
            .filter(|entry| entry.staged.is_some() || entry.unstaged.is_some())
            .map(|entry| entry.path.clone())
            .collect::<Vec<_>>();
        all_files.sort();
        all_files.dedup();
        let mut staged_files = snapshot
            .entries
            .iter()
            .filter(|entry| entry.staged.is_some())
            .map(|entry| entry.path.clone())
            .collect::<Vec<_>>();
        staged_files.sort();
        staged_files.dedup();

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

        Ok(RemoteShipSource {
            repo,
            branch,
            needs_upstream,
            all_files,
            staged_files,
            conversation_files,
            solo: agent.is_solo(),
        })
    }

    pub(in crate::ui::center) fn remote_ship_preview(
        &mut self,
        agent_id: Uuid,
        cx: &mut Context<Self>,
    ) -> RemoteResult<ShipPreviewDto> {
        let agent = self
            .agents
            .read(cx)
            .agent(agent_id)
            .cloned()
            .filter(|agent| !agent.hidden_doc_assistant)
            .ok_or_else(|| RemoteError::not_found("agent not found"))?;
        let source = self.remote_ship_source(&agent, cx)?;
        let status = self.remote_ship_status.get(&agent_id);
        let paths = |files: &[PathBuf]| {
            files
                .iter()
                .map(|path| path.to_string_lossy().to_string())
                .collect::<Vec<_>>()
        };
        Ok(ShipPreviewDto {
            branch: source.branch.clone(),
            solo: source.solo,
            needs_upstream: source.needs_upstream,
            has_remote: repo_has_remote(&source.repo),
            default_pr_base: crate::ui::git::git_panel::default_remote_branch(&source.repo),
            conversation_files: paths(&source.conversation_files),
            all_files: paths(&source.all_files),
            shipping: matches!(status, Some(RemoteShipStatus::Shipping { .. })),
            ship_error: match status {
                Some(RemoteShipStatus::Failed { message, .. }) => Some(message.clone()),
                _ => None,
            },
        })
    }

    pub(in crate::ui::center) fn remote_ship_agent_work(
        &mut self,
        agent_id: Uuid,
        request: ShipRequest,
        cx: &mut Context<Self>,
    ) -> RemoteResult<()> {
        let agent = self
            .agents
            .read(cx)
            .agent(agent_id)
            .cloned()
            .filter(|agent| !agent.hidden_doc_assistant)
            .ok_or_else(|| RemoteError::not_found("agent not found"))?;
        if matches!(
            self.remote_ship_status.get(&agent_id),
            Some(RemoteShipStatus::Shipping { .. })
        ) {
            return Err(RemoteError::conflict("a ship is already in progress"));
        }
        let source = self.remote_ship_source(&agent, cx)?;
        let files = match request.scope {
            ShipScopeDto::Conversation => source.conversation_files.clone(),
            ShipScopeDto::All => source.all_files.clone(),
        };
        if files.is_empty() {
            return Err(RemoteError::conflict(
                "no changed files to ship in this scope",
            ));
        }
        let selected = files
            .iter()
            .cloned()
            .collect::<std::collections::HashSet<_>>();
        if source
            .staged_files
            .iter()
            .any(|path| !selected.contains(path))
        {
            return Err(RemoteError::conflict(
                "files outside this scope are staged — unstage them on your Mac first",
            ));
        }
        // A Solo is already on its own branch — never branch off it remotely.
        let create_branch = request.create_branch && !source.solo;
        if source.branch.is_none() && !create_branch {
            return Err(RemoteError::conflict(
                "git HEAD is not on a branch — turn on \"New branch\" to ship",
            ));
        }
        let action = if request.open_pr {
            AgentShipAction::CommitPushPr
        } else if request.push {
            AgentShipAction::CommitPush
        } else {
            AgentShipAction::Commit
        };
        let pr_base_branch = request
            .pr_base_branch
            .as_deref()
            .map(str::trim)
            .filter(|base| !base.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| crate::ui::git::git_panel::default_remote_branch(&source.repo));

        let repo = source.repo.clone();
        let tracked_repo = agent.repository_root().to_path_buf();
        let current_branch = source.branch.clone();
        let needs_upstream = source.needs_upstream;
        let agent_title = agent.title.clone();
        let project_id = agent.project_id;
        let generation_agent = self.workspace.read(cx).generation_agent.clone();

        self.remote_ship_status.insert(
            agent_id,
            RemoteShipStatus::Shipping {
                started_at: unix_now_secs(),
            },
        );
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let preparation = prepare_agent_ship_content(
                        &generation_agent,
                        &repo,
                        current_branch.as_deref(),
                        create_branch,
                        "",
                        &agent_title,
                        &files,
                        "",
                        &pr_base_branch,
                        "",
                        "",
                        action,
                    )?;
                    let resolved_branch = preparation.branch_name.as_deref().unwrap_or("");
                    let resolved_pr_title = preparation
                        .pr
                        .as_ref()
                        .map(|pr| pr.title.as_str())
                        .unwrap_or("");
                    let resolved_pr_body = preparation
                        .pr
                        .as_ref()
                        .map(|pr| pr.body.as_str())
                        .unwrap_or("");
                    run_agent_ship_operation(
                        &repo,
                        agent_id,
                        project_id,
                        current_branch.as_deref(),
                        create_branch,
                        resolved_branch,
                        needs_upstream,
                        &files,
                        &preparation.commit_message,
                        &pr_base_branch,
                        resolved_pr_title,
                        resolved_pr_body,
                        None,
                        action,
                    )
                })
                .await;

            this.update(cx, |center, cx| {
                match result {
                    Ok(outcome) => {
                        center.remote_ship_status.remove(&agent_id);
                        center.attach_ship_commit_to_changed_files(
                            agent_id,
                            outcome.snapshot_id,
                            outcome.commit_sha.clone(),
                            cx,
                        );
                        center.append_agent_ship_result(agent_id, None, &outcome, cx);
                        if let Some(branch) = outcome.tracked_pr_branch.clone() {
                            center.track_agent_ship_pr_branch(
                                agent_id,
                                tracked_repo.clone(),
                                branch,
                                cx,
                            );
                        }
                        // A Solo's PR is its work leaving home — pack the lane
                        // up (branch and chat survive), same as the dialog.
                        if outcome.pr_url.is_some() {
                            center.finish_solo_ship(agent_id, cx);
                        }
                        if let Some(git) = center.git_states.read(cx).get(project_id) {
                            git.update(cx, |git, cx| {
                                git.last_message = Some(outcome.message.clone());
                                git.last_error = None;
                                git.refresh(cx);
                            });
                        }
                    }
                    Err(error) => {
                        center.remote_ship_status.insert(
                            agent_id,
                            RemoteShipStatus::Failed {
                                message: format!("{error:#}"),
                                at: unix_now_secs(),
                            },
                        );
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        Ok(())
    }
}

fn repo_has_remote(repo: &Path) -> bool {
    std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .arg("remote")
        .output()
        .map(|output| {
            output.status.success() && !String::from_utf8_lossy(&output.stdout).trim().is_empty()
        })
        .unwrap_or(false)
}
