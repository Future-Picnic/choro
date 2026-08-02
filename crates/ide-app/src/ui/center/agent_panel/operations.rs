use super::*;

pub(super) fn prepare_agent_ship_content(
    generation_agent: &ide_core::config::GenerationAgent,
    repo: &Path,
    current_branch: Option<&str>,
    create_branch: bool,
    branch_name: &str,
    agent_title: &str,
    files: &[PathBuf],
    commit_message: &str,
    pr_base_branch: &str,
    pr_title: &str,
    pr_description: &str,
    action: AgentShipAction,
) -> anyhow::Result<AgentShipPreparation> {
    let commit_message = if commit_message.trim().is_empty() {
        crate::ui::git::git_panel::generate_commit_message_for_files(generation_agent, repo, files)?
    } else {
        commit_message.trim().to_string()
    };

    let prompt_branch = if create_branch {
        if branch_name.trim().is_empty() {
            unique_agent_ship_branch_name(repo, agent_title, &commit_message, pr_title)?
        } else {
            branch_name.trim().to_string()
        }
    } else {
        current_branch
            .map(str::to_string)
            .ok_or_else(|| anyhow::anyhow!("Current Git HEAD is not on a branch"))?
    };

    let pr = if matches!(action, AgentShipAction::CommitPushPr) {
        let pr_base_branch = pr_base_branch.trim();
        if pr_base_branch.is_empty() {
            anyhow::bail!("Choose a base branch for the pull request");
        }
        let generated = if pr_title.trim().is_empty() || pr_description.trim().is_empty() {
            Some(crate::ui::git::git_panel::generate_pull_request_for_files(
                generation_agent,
                repo,
                &prompt_branch,
                pr_base_branch,
                files,
                &commit_message,
            )?)
        } else {
            None
        };
        Some(crate::ui::git::git_panel::GeneratedPullRequest {
            title: if pr_title.trim().is_empty() {
                generated
                    .as_ref()
                    .map(|pr| pr.title.clone())
                    .unwrap_or_else(|| commit_message.lines().next().unwrap_or("Update").into())
            } else {
                pr_title.trim().to_string()
            },
            body: if pr_description.trim().is_empty() {
                generated
                    .as_ref()
                    .map(|pr| pr.body.clone())
                    .unwrap_or_else(|| "## Summary\n- \n\n## Testing\n- Not run".to_string())
            } else {
                pr_description.trim().to_string()
            },
        })
    } else {
        None
    };

    let branch_name = if create_branch {
        let requested = if branch_name.trim().is_empty() {
            unique_agent_ship_branch_name(
                repo,
                agent_title,
                &commit_message,
                pr.as_ref().map(|pr| pr.title.as_str()).unwrap_or(pr_title),
            )?
        } else {
            branch_name.trim().to_string()
        };
        validate_agent_ship_branch_name(repo, &requested)?;
        Some(requested)
    } else {
        None
    };

    Ok(AgentShipPreparation {
        branch_name,
        commit_message,
        pr,
    })
}

pub(super) fn run_agent_ship_operation(
    repo: &Path,
    agent_id: Uuid,
    project_id: ProjectId,
    current_branch: Option<&str>,
    create_branch: bool,
    branch_name: &str,
    needs_upstream: bool,
    files: &[PathBuf],
    commit_message: &str,
    pr_base_branch: &str,
    pr_title: &str,
    pr_description: &str,
    action: AgentShipAction,
) -> anyhow::Result<AgentShipOutcome> {
    let branch = if create_branch {
        let requested = branch_name.trim().to_string();
        validate_agent_ship_branch_name(repo, &requested)?;
        ide_core::git::write::create_branch(repo, &requested)?;
        requested
    } else {
        current_branch
            .map(str::to_string)
            .ok_or_else(|| anyhow::anyhow!("Current Git HEAD is not on a branch"))?
    };

    if commit_message.trim().is_empty() {
        anyhow::bail!("Generate or enter a commit message before shipping");
    }
    if matches!(action, AgentShipAction::CommitPushPr)
        && (pr_title.trim().is_empty() || pr_description.trim().is_empty())
    {
        anyhow::bail!("Generate or enter pull request title and description before shipping");
    }
    if matches!(action, AgentShipAction::CommitPushPr) && pr_base_branch.trim().is_empty() {
        anyhow::bail!("Choose a base branch for the pull request");
    }

    let snapshot_id = match capture_agent_ship_diff_snapshot(repo, agent_id, project_id, files) {
        Ok(snapshot_id) => snapshot_id,
        Err(error) => {
            eprintln!("failed to capture pre-commit diff snapshot: {error:#}");
            None
        }
    };
    let refs = files.iter().map(|path| path.as_path()).collect::<Vec<_>>();
    ide_core::git::write::stage(repo, &refs)?;
    let commit_message = commit_message.trim().to_string();
    let commit = ide_core::git::write::commit(repo, &commit_message)?;
    if let Some(snapshot_id) = snapshot_id {
        if let Ok(store) = ide_core::local_store::LocalStore::open_default() {
            if let Err(error) = store.update_agent_diff_snapshot_commit(snapshot_id, &commit.sha) {
                eprintln!("failed to attach commit SHA to diff snapshot: {error:#}");
            }
        }
    }

    if matches!(
        action,
        AgentShipAction::CommitPush | AgentShipAction::CommitPushPr
    ) {
        let output =
            ide_core::git::remote::push(repo, Some(&branch), needs_upstream || create_branch)?;
        if !output.success {
            anyhow::bail!("{}", output.message());
        }
    }

    let pull_request = if matches!(action, AgentShipAction::CommitPushPr) {
        let pull_request = crate::ui::git::git_panel::GeneratedPullRequest {
            title: pr_title.trim().to_string(),
            body: pr_description.trim().to_string(),
        };
        let pr_url = crate::ui::git::git_panel::create_pull_request_with_gh(
            repo,
            &branch,
            pr_base_branch,
            &pull_request,
        )?;
        Some((pr_url, pull_request))
    } else {
        None
    };
    let pr_url = pull_request.as_ref().map(|(url, _)| url.clone());
    let pr_title = pull_request
        .as_ref()
        .map(|(_, pull_request)| pull_request.title.clone());
    let pr_body = pull_request
        .as_ref()
        .map(|(_, pull_request)| pull_request.body.clone());

    let message = match action {
        AgentShipAction::Commit => format!("Committed {}", commit.sha_short),
        AgentShipAction::CommitPush => {
            format!("Committed {} and pushed {branch}", commit.sha_short)
        }
        AgentShipAction::CommitPushPr => {
            format!("Committed {}, pushed {branch}, opened PR", commit.sha_short)
        }
    };
    Ok(AgentShipOutcome {
        message,
        action: AgentShipDialog::action_label(action).to_string(),
        branch: branch.clone(),
        pr_url,
        pr_title,
        pr_body,
        tracked_pr_branch: matches!(action, AgentShipAction::CommitPushPr).then_some(branch),
        snapshot_id,
        commit_sha: commit.sha,
    })
}

pub(super) fn capture_agent_ship_diff_snapshot(
    repo: &Path,
    agent_id: Uuid,
    project_id: ProjectId,
    files: &[PathBuf],
) -> anyhow::Result<Option<Uuid>> {
    let selected = files
        .iter()
        .map(|path| normalize_agent_ship_path(repo, path))
        .collect::<std::collections::HashSet<_>>();
    if selected.is_empty() {
        return Ok(None);
    }
    let diffs = ide_core::git::worktree_diffs(repo)?
        .into_iter()
        .filter(|diff| selected.contains(&normalize_agent_ship_path(repo, &diff.path)))
        .collect::<Vec<_>>();
    if diffs.is_empty() {
        return Ok(None);
    }
    let snapshot = ide_core::local_store::LocalStore::open_default()?.create_agent_diff_snapshot(
        agent_id,
        project_id,
        repo.to_path_buf(),
        "ship_pre_commit",
        git_head_sha(repo),
        None,
        None,
        diffs,
    )?;
    Ok(Some(snapshot.id))
}

pub(super) fn git_head_sha(repo: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub(super) fn agent_ship_pr_status_colors(
    pr: &crate::ui::git::git_panel::BranchPullRequest,
    cx: &mut Context<CenterArea>,
) -> (gpui::Hsla, gpui::Hsla) {
    // Same GitHub status palette as the git panel, so the ship badge matches the
    // PR colours everywhere (open=green, merged=purple, closed=red, draft=grey).
    let (_, color) = crate::ui::git::git_panel::pull_request_status_style(pr, cx);
    (color, color)
}

pub(super) fn validate_agent_ship_branch_name(repo: &Path, name: &str) -> anyhow::Result<()> {
    if name.trim().is_empty() {
        anyhow::bail!("Branch name cannot be empty");
    }
    let output = std::process::Command::new("git")
        .args(["check-ref-format", "--branch", name])
        .current_dir(repo)
        .output()?;
    if !output.status.success() {
        anyhow::bail!("Invalid branch name: {name}");
    }
    Ok(())
}

pub(super) fn agent_ship_branch_exists(repo: &Path, name: &str) -> bool {
    std::process::Command::new("git")
        .args([
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/heads/{name}"),
        ])
        .current_dir(repo)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

pub(super) fn unique_agent_ship_branch_name(
    repo: &Path,
    agent_title: &str,
    commit_message: &str,
    pr_title: &str,
) -> anyhow::Result<String> {
    let seed = [pr_title, commit_message, agent_title]
        .into_iter()
        .map(str::trim)
        .find(|value| !value.is_empty())
        .unwrap_or("agent work");
    let slug = agent_ship_slug(seed);
    let base = slug;
    validate_agent_ship_branch_name(repo, &base)?;
    if !agent_ship_branch_exists(repo, &base) {
        return Ok(base);
    }
    for index in 2..100 {
        let candidate = format!("{base}-{index}");
        if !agent_ship_branch_exists(repo, &candidate) {
            return Ok(candidate);
        }
    }
    anyhow::bail!("Could not find an available branch name for {base}");
}

pub(super) fn agent_ship_slug(seed: &str) -> String {
    let mut out = String::new();
    let mut pending_dash = false;
    for ch in seed.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            out.push(ch.to_ascii_lowercase());
            pending_dash = false;
        } else {
            pending_dash = true;
        }
        if out.len() >= 42 {
            break;
        }
    }
    let slug = out.trim_matches('-').trim().to_string();
    if slug.is_empty() {
        "agent-work".to_string()
    } else {
        slug
    }
}

pub(super) fn normalize_agent_ship_path(project_path: &Path, path: &Path) -> PathBuf {
    let relative = if path.is_absolute() {
        path.strip_prefix(project_path).unwrap_or(path)
    } else {
        path
    };
    relative
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => Some(PathBuf::from(value)),
            _ => None,
        })
        .fold(PathBuf::new(), |mut acc, component| {
            acc.push(component);
            acc
        })
}
