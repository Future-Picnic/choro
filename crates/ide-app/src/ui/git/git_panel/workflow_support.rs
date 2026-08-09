use super::*;

use ide_core::{GitWorkflowCompletionPolicy, GitWorkflowRunState};

pub(super) fn repository_relative_path(
    project_root: &Path,
    repository: &Path,
) -> anyhow::Result<PathBuf> {
    let relative = repository.strip_prefix(project_root).map_err(|_| {
        anyhow::anyhow!(
            "Repository {} is outside project {}",
            repository.display(),
            project_root.display()
        )
    })?;
    let relative = if relative.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        relative.to_path_buf()
    };
    ide_core::validate_repository_relative_path(&relative)?;
    Ok(relative)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct RemoteWorkflowBranch {
    pub(super) name: String,
    pub(super) sha: String,
    pub(super) protected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct WorkflowComparison {
    pub(super) ahead_by: usize,
    pub(super) behind_by: usize,
    pub(super) total_commits: usize,
    pub(super) changed_files: usize,
    pub(super) source_sha: String,
    pub(super) commit_summaries: Vec<String>,
}

impl WorkflowComparison {
    pub(super) fn has_commits_to_merge(&self) -> bool {
        self.ahead_by > 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct WorkflowReview {
    pub(super) comparison: WorkflowComparison,
    pub(super) existing_pull_request: Option<BranchPullRequest>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct WorkflowReconciliation {
    pub(super) state: GitWorkflowRunState,
    pub(super) error: Option<String>,
}

pub(super) fn missing_workflow_branch(
    branches: &[RemoteWorkflowBranch],
    source: &str,
    destination: &str,
) -> Option<String> {
    let source_missing = !source.is_empty() && !branches.iter().any(|branch| branch.name == source);
    let destination_missing =
        !destination.is_empty() && !branches.iter().any(|branch| branch.name == destination);
    match (source_missing, destination_missing) {
        (true, true) => Some(format!(
            "Source branch '{source}' and destination branch '{destination}' no longer exist on GitHub. Edit the workflow to choose live remote branches."
        )),
        (true, false) => Some(format!(
            "Source branch '{source}' no longer exists on GitHub. Edit the workflow to choose a live remote branch."
        )),
        (false, true) => Some(format!(
            "Destination branch '{destination}' no longer exists on GitHub. Edit the workflow to choose a live remote branch."
        )),
        (false, false) => None,
    }
}

pub(super) fn auto_merge_unavailable(error: &anyhow::Error) -> WorkflowReconciliation {
    WorkflowReconciliation {
        state: GitWorkflowRunState::NeedsAttention,
        error: Some(format!(
            "Auto-merge is unavailable: {error:#}. Merge with confirmation or edit the workflow."
        )),
    }
}

#[derive(serde::Deserialize)]
struct GithubBranch {
    name: String,
    commit: GithubBranchCommit,
    #[serde(default)]
    protected: bool,
}

#[derive(serde::Deserialize)]
struct GithubBranchCommit {
    sha: String,
}

#[derive(serde::Deserialize)]
struct GithubComparison {
    #[serde(default)]
    ahead_by: usize,
    #[serde(default)]
    behind_by: usize,
    #[serde(default)]
    total_commits: usize,
    #[serde(default)]
    files: Vec<serde_json::Value>,
    #[serde(default)]
    commits: Vec<GithubComparisonCommit>,
}

#[derive(serde::Deserialize)]
struct GithubComparisonCommit {
    commit: GithubCommitDetails,
}

#[derive(serde::Deserialize)]
struct GithubCommitDetails {
    message: String,
}

fn gh_output_error(output: &std::process::Output, fallback: &str) -> anyhow::Error {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    anyhow::anyhow!(if stderr.is_empty() {
        fallback.to_string()
    } else {
        stderr
    })
}

fn repository_identity(repo: &Path) -> anyhow::Result<String> {
    let output = gh_command_for_repo(repo)?
        .args([
            "repo",
            "view",
            "--json",
            "nameWithOwner",
            "--jq",
            ".nameWithOwner",
        ])
        .current_dir(repo)
        .env("GH_PROMPT_DISABLED", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()?;
    if !output.status.success() {
        return Err(gh_output_error(
            &output,
            "Git Workflows currently require a GitHub repository",
        ));
    }
    let identity = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if identity.is_empty() {
        anyhow::bail!("GitHub did not return the repository identity");
    }
    Ok(identity)
}

fn parse_paginated_branches(bytes: &[u8]) -> anyhow::Result<Vec<RemoteWorkflowBranch>> {
    let pages: Vec<Vec<GithubBranch>> = match serde_json::from_slice(bytes) {
        Ok(pages) => pages,
        Err(_) => vec![serde_json::from_slice::<Vec<GithubBranch>>(bytes)?],
    };
    let mut branches: Vec<RemoteWorkflowBranch> = pages
        .into_iter()
        .flatten()
        .map(|branch| RemoteWorkflowBranch {
            name: branch.name,
            sha: branch.commit.sha,
            protected: branch.protected,
        })
        .collect();
    branches.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(branches)
}

pub(super) fn list_remote_workflow_branches(
    repo: &Path,
) -> anyhow::Result<Vec<RemoteWorkflowBranch>> {
    let identity = repository_identity(repo)?;
    let endpoint = format!("repos/{identity}/branches?per_page=100");
    let output = gh_command_for_repo(repo)?
        .args(["api", "--paginate", "--slurp", &endpoint])
        .current_dir(repo)
        .env("GH_PROMPT_DISABLED", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()?;
    if !output.status.success() {
        return Err(gh_output_error(&output, "Could not load GitHub branches"));
    }
    parse_paginated_branches(&output.stdout)
}

fn parse_comparison(bytes: &[u8], source_sha: String) -> anyhow::Result<WorkflowComparison> {
    let comparison = serde_json::from_slice::<GithubComparison>(bytes)?;
    Ok(WorkflowComparison {
        ahead_by: comparison.ahead_by,
        behind_by: comparison.behind_by,
        total_commits: comparison.total_commits,
        changed_files: comparison.files.len(),
        source_sha,
        commit_summaries: comparison
            .commits
            .into_iter()
            .map(|commit| {
                commit
                    .commit
                    .message
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .to_string()
            })
            .filter(|summary| !summary.is_empty())
            .collect(),
    })
}

pub(super) fn compare_remote_workflow_branches(
    repo: &Path,
    source: &str,
    destination: &str,
) -> anyhow::Result<WorkflowComparison> {
    if source.trim().is_empty() || destination.trim().is_empty() || source == destination {
        anyhow::bail!("Choose two different remote branches");
    }
    let identity = repository_identity(repo)?;
    let source_endpoint = format!(
        "repos/{identity}/branches/{}",
        pull_request_support::url_encode(source)
    );
    let source_output = gh_command_for_repo(repo)?
        .args(["api", &source_endpoint, "--jq", ".commit.sha"])
        .current_dir(repo)
        .env("GH_PROMPT_DISABLED", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()?;
    if !source_output.status.success() {
        return Err(gh_output_error(
            &source_output,
            &format!("Source branch '{source}' no longer exists on GitHub"),
        ));
    }
    let source_sha = String::from_utf8_lossy(&source_output.stdout)
        .trim()
        .to_string();
    if source_sha.is_empty() {
        anyhow::bail!("GitHub did not return the source branch commit");
    }

    let route = format!(
        "{}...{}",
        pull_request_support::url_encode(destination),
        pull_request_support::url_encode(source)
    );
    let endpoint = format!("repos/{identity}/compare/{route}");
    let output = gh_command_for_repo(repo)?
        .args(["api", &endpoint])
        .current_dir(repo)
        .env("GH_PROMPT_DISABLED", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()?;
    if !output.status.success() {
        return Err(gh_output_error(
            &output,
            "Could not compare the remote branches on GitHub",
        ));
    }
    parse_comparison(&output.stdout, source_sha)
}

pub(super) fn exact_open_pull_request(
    repo: &Path,
    source: &str,
    destination: &str,
) -> anyhow::Result<Option<BranchPullRequest>> {
    let output = gh_command_for_repo(repo)?
        .args([
            "pr",
            "list",
            "--head",
            source,
            "--base",
            destination,
            "--state",
            "open",
            "--limit",
            "20",
            "--json",
            "number,title,url,state,isDraft,headRefName,baseRefName,headRefOid,mergeStateStatus,reviewDecision,statusCheckRollup,autoMergeRequest",
        ])
        .current_dir(repo)
        .env("GH_PROMPT_DISABLED", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()?;
    if !output.status.success() {
        return Err(gh_output_error(
            &output,
            "Could not read pull requests from GitHub",
        ));
    }
    let prs = serde_json::from_slice::<Vec<GithubPullRequest>>(&output.stdout)?;
    Ok(prs.into_iter().find_map(|pr| {
        let matches = pr.head_ref_name.as_deref() == Some(source)
            && pr.base_ref_name.as_deref() == Some(destination)
            && pr.state.eq_ignore_ascii_case("open");
        matches.then(|| pull_request_support::branch_pull_request_from_github(pr, source))
    }))
}

pub(super) fn review_remote_workflow(
    repo: &Path,
    source: &str,
    destination: &str,
) -> anyhow::Result<WorkflowReview> {
    Ok(WorkflowReview {
        comparison: compare_remote_workflow_branches(repo, source, destination)?,
        existing_pull_request: exact_open_pull_request(repo, source, destination)?,
    })
}

pub(super) fn deterministic_pull_request_text(
    title: String,
    source: &str,
    destination: &str,
    comparison: &WorkflowComparison,
) -> GeneratedPullRequest {
    let mut body = format!(
        "## Remote branch promotion\n\n- Route: `{source}` → `{destination}`\n- Commits: {}\n- Changed files: {}",
        comparison.total_commits, comparison.changed_files
    );
    if !comparison.commit_summaries.is_empty() {
        body.push_str("\n\n### Commits\n");
        for summary in comparison.commit_summaries.iter().take(12) {
            body.push_str("\n- ");
            body.push_str(summary);
        }
        if comparison.commit_summaries.len() > 12 {
            body.push_str(&format!(
                "\n- …and {} more",
                comparison.commit_summaries.len() - 12
            ));
        }
    }
    body.push_str("\n\n_Created by Choro Git Workflows._");
    GeneratedPullRequest { title, body }
}

pub(super) fn create_or_reuse_workflow_pull_request(
    repo: &Path,
    source: &str,
    destination: &str,
    pull_request: &GeneratedPullRequest,
) -> anyhow::Result<BranchPullRequest> {
    if let Some(existing) = exact_open_pull_request(repo, source, destination)? {
        return Ok(existing);
    }
    create_pull_request_with_gh(repo, source, destination, pull_request)?;
    exact_open_pull_request(repo, source, destination)?.ok_or_else(|| {
        anyhow::anyhow!("GitHub created the pull request but Choro could not read it back")
    })
}

pub(super) fn validate_pull_request_matches_review(
    pr: &BranchPullRequest,
    reviewed_head_sha: Option<&str>,
) -> anyhow::Result<()> {
    let reviewed_head_sha = reviewed_head_sha
        .filter(|sha| !sha.is_empty())
        .ok_or_else(|| anyhow::anyhow!("The reviewed source commit is missing"))?;
    let pull_request_head_sha = pr
        .head_oid
        .as_deref()
        .filter(|sha| !sha.is_empty())
        .ok_or_else(|| anyhow::anyhow!("GitHub did not return the PR head commit"))?;
    if pull_request_head_sha != reviewed_head_sha {
        anyhow::bail!(
            "The source branch changed after review. Review the remote changes again before starting this workflow."
        );
    }
    Ok(())
}

fn read_workflow_pull_request(
    repo: &Path,
    number: u64,
    source: &str,
    destination: &str,
) -> anyhow::Result<BranchPullRequest> {
    let number = number.to_string();
    let output = gh_command_for_repo(repo)?
        .args([
            "pr",
            "view",
            &number,
            "--json",
            "number,title,url,state,isDraft,headRefName,baseRefName,headRefOid,mergeStateStatus,reviewDecision,statusCheckRollup,autoMergeRequest",
        ])
        .current_dir(repo)
        .env("GH_PROMPT_DISABLED", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()?;
    if !output.status.success() {
        return Err(gh_output_error(
            &output,
            &format!("Pull request #{} is no longer available", number),
        ));
    }
    let pr = pull_request_support::branch_pull_request_from_github(
        serde_json::from_slice::<GithubPullRequest>(&output.stdout)?,
        source,
    );
    if pr.branch != source || pr.base_branch != destination {
        anyhow::bail!(
            "Pull request #{} now represents {} → {}, not {} → {}",
            pr.number,
            pr.branch,
            pr.base_branch,
            source,
            destination
        );
    }
    Ok(pr)
}

fn auto_merge_arguments(
    number: u64,
    expected_head_sha: &str,
    method: pull_request_support::GithubMergeMethod,
) -> Vec<String> {
    vec![
        "pr".into(),
        "merge".into(),
        number.to_string(),
        "--auto".into(),
        "--match-head-commit".into(),
        expected_head_sha.into(),
        method.cli_flag().into(),
    ]
}

pub(super) fn enable_workflow_auto_merge(
    repo: &Path,
    number: u64,
    source: &str,
    destination: &str,
    expected_head_sha: &str,
) -> anyhow::Result<BranchPullRequest> {
    let pr = read_workflow_pull_request(repo, number, source, destination)?;
    if !pr.state.eq_ignore_ascii_case("OPEN") {
        anyhow::bail!("Pull request #{} is not open", pr.number);
    }
    if pr.head_oid.as_deref() != Some(expected_head_sha) {
        anyhow::bail!(
            "Pull request #{} source changed; refresh before enabling auto-merge",
            pr.number
        );
    }
    let (_, method) = pull_request_support::repository_identity_and_merge_method(repo)?;
    let args = auto_merge_arguments(number, expected_head_sha, method);
    let output = gh_command_for_repo(repo)?
        .args(args)
        .current_dir(repo)
        .env("GH_PROMPT_DISABLED", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()?;
    if !output.status.success() {
        return Err(gh_output_error(
            &output,
            "GitHub did not allow auto-merge for this repository",
        ));
    }
    read_workflow_pull_request(repo, number, source, destination)
}

pub(super) fn reconcile_workflow_pull_request(
    pr: Option<&BranchPullRequest>,
    expected_head_sha: Option<&str>,
    policy: GitWorkflowCompletionPolicy,
) -> WorkflowReconciliation {
    let Some(pr) = pr else {
        return WorkflowReconciliation {
            state: GitWorkflowRunState::Blocked,
            error: Some("The workflow pull request could not be found on GitHub".into()),
        };
    };
    match pr.state.to_ascii_uppercase().as_str() {
        "MERGED" => {
            return WorkflowReconciliation {
                state: GitWorkflowRunState::Merged,
                error: None,
            };
        }
        "CLOSED" => {
            return WorkflowReconciliation {
                state: GitWorkflowRunState::Closed,
                error: Some("The pull request was closed without merging".into()),
            };
        }
        _ => {}
    }
    if expected_head_sha.is_some_and(|expected| pr.head_oid.as_deref() != Some(expected)) {
        return WorkflowReconciliation {
            state: GitWorkflowRunState::NeedsAttention,
            error: Some("The source branch changed after this workflow started".into()),
        };
    }
    if pr.auto_merge_enabled {
        return WorkflowReconciliation {
            state: GitWorkflowRunState::AutoMergeEnabled,
            error: None,
        };
    }
    if pr.is_draft {
        return WorkflowReconciliation {
            state: GitWorkflowRunState::Blocked,
            error: Some("The pull request is still a draft".into()),
        };
    }
    if pr
        .merge_state_status
        .as_deref()
        .is_some_and(|state| matches!(state, "DIRTY" | "BLOCKED"))
    {
        return WorkflowReconciliation {
            state: GitWorkflowRunState::Blocked,
            error: Some("GitHub reports merge conflicts or a blocking rule".into()),
        };
    }
    if pr.review_decision.as_deref() == Some("CHANGES_REQUESTED") {
        return WorkflowReconciliation {
            state: GitWorkflowRunState::Blocked,
            error: Some("A reviewer requested changes".into()),
        };
    }
    if pr.check_state == PullRequestCheckState::Failing {
        return WorkflowReconciliation {
            state: GitWorkflowRunState::Blocked,
            error: Some("Required checks are failing".into()),
        };
    }
    let checks_ready = matches!(
        pr.check_state,
        PullRequestCheckState::Passing | PullRequestCheckState::Unknown
    );
    let reviews_ready = !matches!(pr.review_decision.as_deref(), Some("REVIEW_REQUIRED"));
    let merge_ready = !pr
        .merge_state_status
        .as_deref()
        .is_some_and(|state| matches!(state, "BEHIND" | "UNKNOWN" | "UNSTABLE"));
    if checks_ready && reviews_ready && merge_ready {
        WorkflowReconciliation {
            state: match policy {
                GitWorkflowCompletionPolicy::ConfirmBeforeMerge => {
                    GitWorkflowRunState::AwaitingConfirmation
                }
                GitWorkflowCompletionPolicy::AutoMergeWhenReady => {
                    GitWorkflowRunState::WaitingForRequirements
                }
            },
            error: None,
        }
    } else {
        WorkflowReconciliation {
            state: GitWorkflowRunState::WaitingForRequirements,
            error: None,
        }
    }
}

fn workflow_pull_request_ready(pr: &BranchPullRequest) -> bool {
    !pr.is_draft
        && matches!(
            pr.check_state,
            PullRequestCheckState::Passing | PullRequestCheckState::Unknown
        )
        && !matches!(
            pr.review_decision.as_deref(),
            Some("REVIEW_REQUIRED" | "CHANGES_REQUESTED")
        )
        && !pr.merge_state_status.as_deref().is_some_and(|state| {
            matches!(
                state,
                "DIRTY" | "BLOCKED" | "BEHIND" | "UNKNOWN" | "UNSTABLE"
            )
        })
}

pub(super) fn refresh_workflow_pull_request(
    repo: &Path,
    number: u64,
    source: &str,
    destination: &str,
) -> anyhow::Result<BranchPullRequest> {
    read_workflow_pull_request(repo, number, source, destination)
}

fn workflow_runs_to_refresh(
    runs: &[GitWorkflowRun],
    repository_path: &Path,
) -> Vec<GitWorkflowRun> {
    runs.iter()
        .filter(|run| run.repository_path == repository_path && !run.state.is_terminal())
        .cloned()
        .collect()
}

impl GitPanel {
    pub(super) fn selected_workflow_context(
        &self,
        cx: &App,
    ) -> Option<(ProjectId, PathBuf, PathBuf)> {
        let project_id = self.workspace.read(cx).active?;
        let project_root = self
            .workspace
            .read(cx)
            .projects
            .iter()
            .find(|project| project.id == project_id)?
            .path
            .clone();
        let repo_path = self.active_git(cx)?.read(cx).repo_path.clone();
        let relative = repository_relative_path(&project_root, &repo_path).ok()?;
        Some((project_id, repo_path, relative))
    }

    pub(super) fn sync_workflow_runs(&mut self, _git: Entity<GitState>, cx: &mut Context<Self>) {
        if !matches!(self.tab, GitTab::Workflows | GitTab::PullRequests)
            || self.workflow_runs_refreshing
        {
            return;
        }
        let stale = self
            .workflow_runs_checked_at
            .is_none_or(|checked_at| checked_at.elapsed() >= WORKFLOW_REFRESH_INTERVAL);
        if !stale {
            return;
        }
        let Some((project_id, repo_path, repository_path)) = self.selected_workflow_context(cx)
        else {
            return;
        };
        let workspace = self.workspace.read(cx);
        let Some(project) = workspace
            .projects
            .iter()
            .find(|project| project.id == project_id)
        else {
            return;
        };
        let policies: HashMap<uuid::Uuid, GitWorkflowCompletionPolicy> = project
            .git_workflows
            .iter()
            .map(|workflow| (workflow.id, workflow.completion_policy))
            .collect();
        let runs = workflow_runs_to_refresh(&project.git_workflow_runs, &repository_path);
        self.workflow_runs_checked_at = Some(Instant::now());
        if runs.is_empty() {
            self.workflow_runs_error = None;
            return;
        }
        self.workflow_runs_refreshing = true;
        self.workflow_runs_error = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let results = cx
                .background_executor()
                .spawn(async move {
                    let mut results = Vec::with_capacity(runs.len());
                    for mut run in runs {
                        let policy = run
                            .workflow_id
                            .and_then(|id| policies.get(&id).copied())
                            .unwrap_or_else(|| {
                                if run.state == GitWorkflowRunState::AutoMergeEnabled {
                                    GitWorkflowCompletionPolicy::AutoMergeWhenReady
                                } else {
                                    GitWorkflowCompletionPolicy::ConfirmBeforeMerge
                                }
                            });
                        let pull_request = match run.pull_request_number {
                            Some(number) => refresh_workflow_pull_request(
                                &repo_path,
                                number,
                                &run.source_branch,
                                &run.destination_branch,
                            )
                            .map(Some),
                            None => exact_open_pull_request(
                                &repo_path,
                                &run.source_branch,
                                &run.destination_branch,
                            ),
                        };
                        match pull_request {
                            Ok(Some(mut pr)) => {
                                let number = pr.number;
                                run.pull_request_number = Some(number);
                                let mut reconciliation = reconcile_workflow_pull_request(
                                    Some(&pr),
                                    run.expected_head_sha.as_deref(),
                                    policy,
                                );
                                if policy == GitWorkflowCompletionPolicy::AutoMergeWhenReady
                                    && reconciliation.state
                                        == GitWorkflowRunState::WaitingForRequirements
                                    && workflow_pull_request_ready(&pr)
                                {
                                    match enable_workflow_auto_merge(
                                        &repo_path,
                                        number,
                                        &run.source_branch,
                                        &run.destination_branch,
                                        run.expected_head_sha.as_deref().unwrap_or_default(),
                                    ) {
                                        Ok(updated_pr) => {
                                            pr = updated_pr;
                                            reconciliation = reconcile_workflow_pull_request(
                                                Some(&pr),
                                                run.expected_head_sha.as_deref(),
                                                policy,
                                            );
                                        }
                                        Err(error) => {
                                            reconciliation = auto_merge_unavailable(&error);
                                        }
                                    }
                                }
                                run.state = reconciliation.state;
                                run.error = reconciliation.error;
                            }
                            Ok(None) => {
                                run.state = GitWorkflowRunState::Failed;
                                run.error = Some(
                                    "No matching open pull request was found after Choro restarted. Review and run the workflow again."
                                        .into(),
                                );
                            }
                            Err(error) => {
                                run.error = Some(format!(
                                    "Last refresh failed; showing the previous state. Retry when GitHub is reachable: {error:#}"
                                ));
                            }
                        }
                        run.updated_at = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map(|duration| duration.as_secs())
                            .unwrap_or(run.updated_at);
                        results.push(run);
                    }
                    results
                })
                .await;
            this.update(cx, |panel, cx| {
                panel.workflow_runs_refreshing = false;
                panel.workflow_runs_error = None;
                let mut persistence_error = None;
                panel.workspace.update(cx, |workspace, cx| {
                    for run in results {
                        if let Err(error) =
                            workspace.upsert_git_workflow_run(project_id, run, cx)
                        {
                            persistence_error = Some(format!("{error:#}"));
                        }
                    }
                });
                panel.workflow_runs_error = persistence_error;
                panel.repo_prs_checked_at = None;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pull_request() -> BranchPullRequest {
        BranchPullRequest {
            branch: "staging".into(),
            base_branch: "production".into(),
            head_oid: Some("abc123".into()),
            number: 42,
            title: "Promote".into(),
            url: "https://github.com/acme/app/pull/42".into(),
            state: "OPEN".into(),
            is_draft: false,
            merge_state_status: Some("CLEAN".into()),
            review_decision: Some("APPROVED".into()),
            check_state: PullRequestCheckState::Passing,
            auto_merge_enabled: false,
        }
    }

    #[test]
    fn parses_paginated_remote_branches() {
        let branches = parse_paginated_branches(
            br#"[[{"name":"staging","commit":{"sha":"abc"},"protected":false}],
                 [{"name":"production","commit":{"sha":"def"},"protected":true}]]"#,
        )
        .unwrap();
        assert_eq!(branches.len(), 2);
        assert_eq!(branches[0].name, "production");
        assert!(branches[0].protected);
        assert_eq!(branches[1].sha, "abc");
    }

    #[test]
    fn deleted_branch_requires_editing_before_a_workflow_can_run() {
        let branches = vec![RemoteWorkflowBranch {
            name: "production".into(),
            sha: "def456".into(),
            protected: true,
        }];
        let error = missing_workflow_branch(&branches, "staging", "production").unwrap();
        assert!(error.contains("Source branch 'staging' no longer exists"));
        assert!(error.contains("Edit the workflow"));
        assert!(missing_workflow_branch(&branches, "production", "production").is_none());
    }

    #[test]
    fn comparison_parser_returns_bounded_review_facts() {
        let comparison = parse_comparison(
            br#"{"ahead_by":2,"behind_by":1,"total_commits":2,
                 "files":[{},{}],"commits":[
                    {"commit":{"message":"First subject\n\nBody"}},
                    {"commit":{"message":"Second subject"}}
                 ]}"#,
            "abc123".into(),
        )
        .unwrap();
        assert_eq!(comparison.ahead_by, 2);
        assert_eq!(comparison.behind_by, 1);
        assert_eq!(comparison.changed_files, 2);
        assert_eq!(
            comparison.commit_summaries,
            ["First subject", "Second subject"]
        );
    }

    #[test]
    fn comparison_with_nothing_ahead_has_no_commits_to_merge() {
        let comparison = parse_comparison(
            br#"{"ahead_by":0,"behind_by":1,"total_commits":0,"files":[],"commits":[]}"#,
            "abc123".into(),
        )
        .unwrap();

        assert!(!comparison.has_commits_to_merge());
    }

    #[test]
    fn deterministic_body_contains_route_counts_and_commit_subjects() {
        let comparison = WorkflowComparison {
            ahead_by: 2,
            behind_by: 0,
            total_commits: 2,
            changed_files: 3,
            source_sha: "abc123".into(),
            commit_summaries: vec!["One".into(), "Two".into()],
        };
        let text = deterministic_pull_request_text(
            "Promote staging".into(),
            "staging",
            "production",
            &comparison,
        );
        assert!(text.body.contains("`staging` → `production`"));
        assert!(text.body.contains("Commits: 2"));
        assert!(text.body.contains("Changed files: 3"));
        assert!(text.body.contains("- One"));
    }

    #[test]
    fn reconciliation_covers_ready_auto_merge_and_external_states() {
        let pr = pull_request();
        assert_eq!(
            reconcile_workflow_pull_request(
                Some(&pr),
                Some("abc123"),
                GitWorkflowCompletionPolicy::ConfirmBeforeMerge,
            )
            .state,
            GitWorkflowRunState::AwaitingConfirmation
        );
        assert_eq!(
            reconcile_workflow_pull_request(
                Some(&pr),
                Some("abc123"),
                GitWorkflowCompletionPolicy::AutoMergeWhenReady,
            )
            .state,
            GitWorkflowRunState::WaitingForRequirements
        );
        let mut auto = pr.clone();
        auto.auto_merge_enabled = true;
        auto.merge_state_status = Some("BLOCKED".into());
        assert_eq!(
            reconcile_workflow_pull_request(
                Some(&auto),
                Some("abc123"),
                GitWorkflowCompletionPolicy::AutoMergeWhenReady,
            )
            .state,
            GitWorkflowRunState::AutoMergeEnabled
        );
        let mut merged = pr.clone();
        merged.state = "MERGED".into();
        assert_eq!(
            reconcile_workflow_pull_request(
                Some(&merged),
                Some("abc123"),
                GitWorkflowCompletionPolicy::ConfirmBeforeMerge,
            )
            .state,
            GitWorkflowRunState::Merged
        );
    }

    #[test]
    fn repeated_reconciliation_is_idempotent() {
        let pr = pull_request();
        let first = reconcile_workflow_pull_request(
            Some(&pr),
            Some("abc123"),
            GitWorkflowCompletionPolicy::ConfirmBeforeMerge,
        );
        let second = reconcile_workflow_pull_request(
            Some(&pr),
            Some("abc123"),
            GitWorkflowCompletionPolicy::ConfirmBeforeMerge,
        );
        assert_eq!(second, first);
    }

    #[test]
    fn reconciliation_blocks_conflicts_failures_and_head_changes() {
        let mut pr = pull_request();
        pr.merge_state_status = Some("DIRTY".into());
        assert_eq!(
            reconcile_workflow_pull_request(
                Some(&pr),
                Some("abc123"),
                GitWorkflowCompletionPolicy::ConfirmBeforeMerge,
            )
            .state,
            GitWorkflowRunState::Blocked
        );
        let mut changed = pull_request();
        changed.head_oid = Some("def456".into());
        assert_eq!(
            reconcile_workflow_pull_request(
                Some(&changed),
                Some("abc123"),
                GitWorkflowCompletionPolicy::ConfirmBeforeMerge,
            )
            .state,
            GitWorkflowRunState::NeedsAttention
        );

        let mut failing = pull_request();
        failing.check_state = PullRequestCheckState::Failing;
        assert_eq!(
            reconcile_workflow_pull_request(
                Some(&failing),
                Some("abc123"),
                GitWorkflowCompletionPolicy::ConfirmBeforeMerge,
            )
            .state,
            GitWorkflowRunState::Blocked
        );

        let mut draft = pull_request();
        draft.is_draft = true;
        assert_eq!(
            reconcile_workflow_pull_request(
                Some(&draft),
                Some("abc123"),
                GitWorkflowCompletionPolicy::ConfirmBeforeMerge,
            )
            .state,
            GitWorkflowRunState::Blocked
        );

        let mut closed = pull_request();
        closed.state = "CLOSED".into();
        assert_eq!(
            reconcile_workflow_pull_request(
                Some(&closed),
                Some("abc123"),
                GitWorkflowCompletionPolicy::ConfirmBeforeMerge,
            )
            .state,
            GitWorkflowRunState::Closed
        );

        assert_eq!(
            reconcile_workflow_pull_request(
                None,
                Some("abc123"),
                GitWorkflowCompletionPolicy::ConfirmBeforeMerge,
            )
            .state,
            GitWorkflowRunState::Blocked
        );
    }

    #[test]
    fn starting_rejects_a_pull_request_head_that_changed_after_review() {
        let pr = pull_request();
        assert!(validate_pull_request_matches_review(&pr, Some("abc123")).is_ok());
        assert!(validate_pull_request_matches_review(&pr, Some("new-head")).is_err());
    }

    #[test]
    fn auto_merge_command_never_bypasses_or_deletes() {
        let args = auto_merge_arguments(
            42,
            "abc123",
            pull_request_support::GithubMergeMethod::Squash,
        );
        assert_eq!(
            args,
            [
                "pr",
                "merge",
                "42",
                "--auto",
                "--match-head-commit",
                "abc123",
                "--squash"
            ]
        );
        assert!(!args.iter().any(|arg| arg == "--admin"));
        assert!(!args.iter().any(|arg| arg == "--delete-branch"));
    }

    #[test]
    fn unsupported_auto_merge_needs_attention_without_falling_back_to_merge() {
        let reconciliation = auto_merge_unavailable(&anyhow::anyhow!(
            "auto-merge is not enabled for this repository"
        ));
        assert_eq!(reconciliation.state, GitWorkflowRunState::NeedsAttention);
        let error = reconciliation.error.unwrap();
        assert!(error.contains("Auto-merge is unavailable"));
        assert!(error.contains("Merge with confirmation or edit the workflow"));
    }

    #[test]
    fn repository_paths_are_project_relative_and_root_is_dot() {
        assert_eq!(
            repository_relative_path(Path::new("/work/app"), Path::new("/work/app")).unwrap(),
            PathBuf::from(".")
        );
        assert_eq!(
            repository_relative_path(Path::new("/work/app"), Path::new("/work/app/services/api"))
                .unwrap(),
            PathBuf::from("services/api")
        );
        assert!(
            repository_relative_path(Path::new("/work/app"), Path::new("/work/other")).is_err()
        );
    }

    #[test]
    fn creating_runs_without_pull_request_numbers_are_refreshed_after_restart() {
        let run = GitWorkflowRun {
            id: uuid::Uuid::new_v4(),
            workflow_id: None,
            repository_path: PathBuf::from("."),
            source_branch: "staging".into(),
            destination_branch: "production".into(),
            pull_request_number: None,
            expected_head_sha: Some("abc123".into()),
            state: GitWorkflowRunState::CreatingPullRequest,
            error: None,
            started_at: 1,
            updated_at: 1,
        };

        let selected = workflow_runs_to_refresh(&[run.clone()], Path::new("."));
        assert_eq!(selected, vec![run]);
    }

    #[test]
    #[ignore = "mutates only an explicitly configured disposable GitHub acceptance repository"]
    fn live_github_acceptance_suite() {
        let require = |name: &str| {
            std::env::var(name).unwrap_or_else(|_| {
                panic!("{name} must be set for the live GitHub acceptance suite")
            })
        };
        assert_eq!(
            require("CHORO_GIT_WORKFLOW_ACCEPTANCE_ALLOW_MUTATION"),
            "1",
            "set CHORO_GIT_WORKFLOW_ACCEPTANCE_ALLOW_MUTATION=1 only for a disposable fixture"
        );
        let repo = PathBuf::from(require("CHORO_GIT_WORKFLOW_ACCEPTANCE_REPO"));
        let source = require("CHORO_GIT_WORKFLOW_ACCEPTANCE_SOURCE");
        let destination = require("CHORO_GIT_WORKFLOW_ACCEPTANCE_DESTINATION");
        let mode =
            std::env::var("CHORO_GIT_WORKFLOW_ACCEPTANCE_MODE").unwrap_or_else(|_| "review".into());

        let branches = list_remote_workflow_branches(&repo).unwrap();
        assert!(branches.iter().any(|branch| branch.name == source));
        assert!(branches.iter().any(|branch| branch.name == destination));
        if let Ok(missing) = std::env::var("CHORO_GIT_WORKFLOW_ACCEPTANCE_MISSING_BRANCH") {
            assert!(!branches.iter().any(|branch| branch.name == missing));
            assert!(missing_workflow_branch(&branches, &missing, &destination).is_some());
        }
        if let Ok(second_repo) = std::env::var("CHORO_GIT_WORKFLOW_ACCEPTANCE_SECOND_REPO") {
            assert!(!list_remote_workflow_branches(Path::new(&second_repo))
                .unwrap()
                .is_empty());
        }

        let review = review_remote_workflow(&repo, &source, &destination).unwrap();
        assert!(
            review.comparison.ahead_by > 0,
            "the fixture source must contain a commit not in the destination"
        );
        if std::env::var("CHORO_GIT_WORKFLOW_ACCEPTANCE_EXPECT_NEW_PR").as_deref() == Ok("1") {
            assert!(review.existing_pull_request.is_none());
        }
        let text = deterministic_pull_request_text(
            format!("Choro acceptance: {source} to {destination}"),
            &source,
            &destination,
            &review.comparison,
        );
        let first =
            create_or_reuse_workflow_pull_request(&repo, &source, &destination, &text).unwrap();
        validate_pull_request_matches_review(&first, Some(&review.comparison.source_sha)).unwrap();
        let reused =
            create_or_reuse_workflow_pull_request(&repo, &source, &destination, &text).unwrap();
        assert_eq!(
            reused.number, first.number,
            "the exact PR route must be reused"
        );

        let first_reconciliation = reconcile_workflow_pull_request(
            Some(&reused),
            Some(&review.comparison.source_sha),
            GitWorkflowCompletionPolicy::ConfirmBeforeMerge,
        );
        let refreshed =
            refresh_workflow_pull_request(&repo, reused.number, &source, &destination).unwrap();
        let restarted_reconciliation = reconcile_workflow_pull_request(
            Some(&refreshed),
            Some(&review.comparison.source_sha),
            GitWorkflowCompletionPolicy::ConfirmBeforeMerge,
        );
        assert_eq!(restarted_reconciliation, first_reconciliation);

        if let Ok(expected) = std::env::var("CHORO_GIT_WORKFLOW_ACCEPTANCE_EXPECTED_STATE") {
            assert_eq!(
                serde_json::to_value(first_reconciliation.state)
                    .unwrap()
                    .as_str(),
                Some(expected.as_str())
            );
        }

        match mode.as_str() {
            "review" => {}
            "confirm_merge" => {
                let outcome = merge_pull_request_with_gh(
                    &repo,
                    &reused.number.to_string(),
                    Some(&destination),
                    Some(&review.comparison.source_sha),
                )
                .unwrap();
                assert_eq!(outcome.number, reused.number);
                let merged =
                    refresh_workflow_pull_request(&repo, reused.number, &source, &destination)
                        .unwrap();
                assert_eq!(
                    reconcile_workflow_pull_request(
                        Some(&merged),
                        Some(&review.comparison.source_sha),
                        GitWorkflowCompletionPolicy::ConfirmBeforeMerge,
                    )
                    .state,
                    GitWorkflowRunState::Merged
                );
            }
            "auto_merge" => {
                let updated = enable_workflow_auto_merge(
                    &repo,
                    reused.number,
                    &source,
                    &destination,
                    &review.comparison.source_sha,
                )
                .unwrap();
                assert!(
                    updated.state.eq_ignore_ascii_case("MERGED") || updated.auto_merge_enabled,
                    "GitHub must either merge immediately or report auto-merge enabled"
                );
            }
            other => panic!("unsupported CHORO_GIT_WORKFLOW_ACCEPTANCE_MODE: {other}"),
        }
    }
}
