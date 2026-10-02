use super::*;

pub(super) fn gh_output(command: &mut Command) -> anyhow::Result<std::process::Output> {
    ide_core::process::output_with_timeout(command, Duration::from_secs(60))
        .map_err(|error| anyhow::anyhow!("GitHub request failed: {error:#}. Check your connection and GitHub authentication, then retry."))
}

pub(super) fn url_encode(input: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push(HEX[(byte >> 4) as usize] as char);
            out.push(HEX[(byte & 0x0f) as usize] as char);
        }
    }
    out
}

pub(super) fn github_repo_from_remote(remote: &str) -> Option<(String, String)> {
    let remote = remote.trim().trim_end_matches(".git");
    let path = remote
        .strip_prefix("git@github.com:")
        .or_else(|| remote.strip_prefix("https://github.com/"))
        .or_else(|| remote.strip_prefix("http://github.com/"))
        .or_else(|| remote.strip_prefix("ssh://git@github.com/"))?;
    let mut parts = path.split('/');
    let owner = parts.next()?.to_string();
    let repo = parts.next()?.to_string();
    (!owner.is_empty() && !repo.is_empty()).then_some((owner, repo))
}

pub(crate) fn github_pull_request_url(
    repo: &Path,
    branch: &str,
    base_branch: &str,
) -> Option<String> {
    let remote = git_output(repo, &["config", "--get", "remote.origin.url"]).ok()?;
    let (owner, repo) = github_repo_from_remote(&remote)?;
    let owner = url_encode(&owner);
    let repo = url_encode(&repo);
    let base_branch = url_encode(base_branch.trim());
    let branch = url_encode(branch);
    Some(format!(
        "https://github.com/{owner}/{repo}/compare/{base_branch}...{branch}?expand=1"
    ))
}

pub(super) fn branch_from_push_message(message: &str) -> Option<PushNoticeEvent> {
    if let Some(branch) = message
        .strip_prefix("Published ")
        .and_then(|rest| rest.strip_suffix(" to origin"))
    {
        return Some(PushNoticeEvent {
            kind: PushNoticeKind::Published,
            branch: branch.to_string(),
        });
    }

    message
        .strip_prefix("Pushed ")
        .and_then(|rest| rest.strip_suffix(" to origin"))
        .map(|branch| PushNoticeEvent {
            kind: PushNoticeKind::Pushed,
            branch: branch.to_string(),
        })
}

pub(super) fn existing_pull_request_url(repo: &Path, branch: &str) -> Option<String> {
    let output = gh_output(
        gh_command_for_repo(repo)
            .ok()?
            .args(["pr", "view", branch, "--json", "url", "--jq", ".url"])
            .current_dir(repo)
            .env("GH_PROMPT_DISABLED", "1")
            .env("GIT_TERMINAL_PROMPT", "0"),
    )
    .ok()?;
    if !output.status.success() {
        return None;
    }
    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !url.is_empty() {
        return Some(url);
    }

    let output = gh_output(
        gh_command_for_repo(repo)
            .ok()?
            .args([
                "pr", "list", "--head", branch, "--state", "all", "--limit", "1", "--json", "url",
                "--jq", ".[0].url",
            ])
            .current_dir(repo)
            .env("GH_PROMPT_DISABLED", "1")
            .env("GIT_TERMINAL_PROMPT", "0"),
    )
    .ok()?;
    if !output.status.success() {
        return None;
    }
    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!url.is_empty() && url != "null").then_some(url)
}

pub(super) fn existing_pull_request_url_for_base(
    repo: &Path,
    branch: &str,
    base_branch: &str,
) -> anyhow::Result<Option<String>> {
    let output = gh_output(
        gh_command_for_repo(repo)?
            .args([
                "pr",
                "list",
                "--head",
                branch,
                "--base",
                base_branch,
                "--state",
                "open",
                "--limit",
                "1",
                "--json",
                "url",
                "--jq",
                ".[0].url",
            ])
            .current_dir(repo)
            .env("GH_PROMPT_DISABLED", "1")
            .env("GIT_TERMINAL_PROMPT", "0"),
    )?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "Could not check for an existing pull request: {}",
            stderr.trim()
        );
    }
    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok((!url.is_empty() && url != "null").then_some(url))
}

pub(crate) fn create_pull_request_with_gh(
    repo: &Path,
    branch: &str,
    base_branch: &str,
    pull_request: &GeneratedPullRequest,
) -> anyhow::Result<String> {
    let base_branch = base_branch.trim();
    if base_branch.is_empty() {
        anyhow::bail!("Choose a base branch for the pull request");
    }
    if let Some(url) = existing_pull_request_url_for_base(repo, branch, base_branch)? {
        return Ok(url);
    }

    let output = gh_output(
        gh_command_for_repo(repo)?
            .args([
                "pr",
                "create",
                "--head",
                branch,
                "--base",
                base_branch,
                "--title",
                pull_request.title.trim(),
                "--body",
                pull_request.body.trim(),
            ])
            .current_dir(repo)
            .env("GH_PROMPT_DISABLED", "1")
            .env("GIT_TERMINAL_PROMPT", "0"),
    )?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if let Some(url) = stdout
            .split_whitespace()
            .find(|part| part.starts_with("https://github.com/"))
        {
            return Ok(url.trim().to_string());
        }
        if let Some(url) = existing_pull_request_url_for_base(repo, branch, base_branch)? {
            return Ok(url);
        }
        anyhow::bail!("GitHub CLI created the pull request but did not return a URL");
    }

    if let Some(url) = existing_pull_request_url_for_base(repo, branch, base_branch)? {
        return Ok(url);
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    anyhow::bail!(if stderr.is_empty() {
        "failed to create pull request with gh".to_string()
    } else {
        stderr
    })
}

pub(super) fn status_value(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .or_else(|| {
            value
                .get(key)
                .and_then(|value| value.get("state").or_else(|| value.get("conclusion")))
                .and_then(|value| value.as_str())
                .map(str::to_string)
        })
}

pub(super) fn pull_request_check_state(rollup: &[serde_json::Value]) -> PullRequestCheckState {
    if rollup.is_empty() {
        return PullRequestCheckState::Unknown;
    }

    let mut pending = false;
    let mut saw_success = false;
    for check in rollup {
        let conclusion = status_value(check, "conclusion")
            .map(|value| value.to_ascii_uppercase())
            .unwrap_or_default();
        let status = status_value(check, "status")
            .map(|value| value.to_ascii_uppercase())
            .unwrap_or_default();

        if matches!(
            conclusion.as_str(),
            "FAILURE" | "FAILED" | "ERROR" | "CANCELLED" | "TIMED_OUT" | "ACTION_REQUIRED"
        ) {
            return PullRequestCheckState::Failing;
        }

        if matches!(
            status.as_str(),
            "QUEUED" | "PENDING" | "IN_PROGRESS" | "WAITING" | "REQUESTED" | "EXPECTED"
        ) || conclusion.is_empty()
        {
            pending = true;
        }

        if matches!(conclusion.as_str(), "SUCCESS" | "SKIPPED" | "NEUTRAL") {
            saw_success = true;
        }
    }

    if pending {
        PullRequestCheckState::Pending
    } else if saw_success {
        PullRequestCheckState::Passing
    } else {
        PullRequestCheckState::Unknown
    }
}

/// A GitHub Primer status colour, picking the dark- or light-mode value to suit
/// the current theme so it still reads like GitHub on either.
pub(super) fn github_status_color(cx: &App, dark: u32, light: u32) -> Hsla {
    let hex = if crate::ui::design::base(cx).l < 0.5 {
        dark
    } else {
        light
    };
    gpui::rgb(hex).into()
}

/// The label and colour for a pull request, using GitHub's own status palette:
/// open = green, merged = purple, closed = red, draft = grey, with CI/merge
/// problems surfaced in red and pending checks in yellow. Shared by the agent
/// header badge and both git-panel PR views so they stay consistent.
pub(crate) fn pull_request_status_style(pr: &BranchPullRequest, cx: &App) -> (SharedString, Hsla) {
    let green = github_status_color(cx, 0x3f_b950, 0x1a_7f37);
    let purple = github_status_color(cx, 0xa3_71f7, 0x82_50df);
    let red = github_status_color(cx, 0xf8_5149, 0xcf_222e);
    let yellow = github_status_color(cx, 0xd2_9922, 0xbf_8700);
    let gray = github_status_color(cx, 0x8b_949e, 0x6e_7781);

    let state = pr.state.to_ascii_uppercase();
    if state == "MERGED" {
        return ("Merged".into(), purple);
    }
    if state == "CLOSED" {
        return ("Closed".into(), red);
    }
    if pr.is_draft {
        return ("Draft".into(), gray);
    }
    if pr
        .merge_state_status
        .as_deref()
        .is_some_and(|status| matches!(status, "DIRTY" | "BLOCKED"))
    {
        return ("Merge blocked".into(), red);
    }
    if pr
        .review_decision
        .as_deref()
        .is_some_and(|decision| decision == "CHANGES_REQUESTED")
    {
        return ("Changes requested".into(), red);
    }
    match pr.check_state {
        PullRequestCheckState::Failing => ("Checks failing".into(), red),
        PullRequestCheckState::Pending => ("Checks pending".into(), yellow),
        PullRequestCheckState::Passing => {
            if pr
                .review_decision
                .as_deref()
                .is_some_and(|decision| decision == "APPROVED")
            {
                ("Approved".into(), green)
            } else {
                ("Checks passing".into(), green)
            }
        }
        PullRequestCheckState::Unknown => ("Open".into(), green),
    }
}

pub(super) fn branch_pull_request_from_github(
    pr: GithubPullRequest,
    fallback_branch: &str,
) -> BranchPullRequest {
    BranchPullRequest {
        branch: pr
            .head_ref_name
            .unwrap_or_else(|| fallback_branch.to_string()),
        base_branch: pr.base_ref_name.unwrap_or_default(),
        head_oid: pr.head_ref_oid,
        number: pr.number,
        title: pr.title,
        url: pr.url,
        state: pr.state,
        is_draft: pr.is_draft,
        merge_state_status: pr.merge_state_status,
        review_decision: pr.review_decision,
        check_state: pull_request_check_state(&pr.status_check_rollup),
        auto_merge_enabled: pr.auto_merge_request.is_some(),
    }
}

pub(crate) fn branch_pull_request(repo: &Path, branch: &str) -> Option<BranchPullRequest> {
    if branch == default_remote_branch(repo) || matches!(branch, "main" | "master") {
        return None;
    }

    let view_output = gh_output(gh_command_for_repo(repo)
        .ok()?
        .args([
            "pr",
            "view",
            branch,
            "--json",
            "number,title,url,state,isDraft,headRefName,baseRefName,headRefOid,mergeStateStatus,reviewDecision,statusCheckRollup,autoMergeRequest",
        ])
        .current_dir(repo)
        .env("GH_PROMPT_DISABLED", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
    )
        .ok()?;
    if view_output.status.success() {
        if let Ok(pr) = serde_json::from_slice::<GithubPullRequest>(&view_output.stdout) {
            return Some(branch_pull_request_from_github(pr, branch));
        }
    }

    let output = gh_output(gh_command_for_repo(repo)
        .ok()?
        .args([
            "pr",
            "list",
            "--head",
            branch,
            "--state",
            "all",
            "--limit",
            "1",
            "--json",
            "number,title,url,state,isDraft,headRefName,baseRefName,headRefOid,mergeStateStatus,reviewDecision,statusCheckRollup,autoMergeRequest",
        ])
        .current_dir(repo)
        .env("GH_PROMPT_DISABLED", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
    )
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let pr = serde_json::from_slice::<Vec<GithubPullRequest>>(&output.stdout)
        .ok()?
        .into_iter()
        .next()?;
    Some(branch_pull_request_from_github(pr, branch))
}

pub(super) fn repo_pull_requests(repo: &Path) -> anyhow::Result<Vec<BranchPullRequest>> {
    let output = gh_output(gh_command_for_repo(repo)?
        .args([
            "pr",
            "list",
            "--state",
            "all",
            "--limit",
            "30",
            "--json",
            "number,title,url,state,isDraft,headRefName,baseRefName,headRefOid,mergeStateStatus,reviewDecision,statusCheckRollup,autoMergeRequest",
        ])
        .current_dir(repo)
        .env("GH_PROMPT_DISABLED", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
    )?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        anyhow::bail!(if stderr.is_empty() {
            "failed to fetch pull requests with gh".to_string()
        } else {
            stderr
        });
    }

    let prs = serde_json::from_slice::<Vec<GithubPullRequest>>(&output.stdout)?;
    Ok(prs
        .into_iter()
        .map(|pr| branch_pull_request_from_github(pr, ""))
        .collect())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GithubMergeMethod {
    Merge,
    Rebase,
    Squash,
}

impl GithubMergeMethod {
    pub(super) fn api_value(self) -> &'static str {
        match self {
            Self::Merge => "merge",
            Self::Rebase => "rebase",
            Self::Squash => "squash",
        }
    }

    pub(super) fn cli_flag(self) -> &'static str {
        match self {
            Self::Merge => "--merge",
            Self::Rebase => "--rebase",
            Self::Squash => "--squash",
        }
    }
}

#[derive(serde::Deserialize)]
struct GithubRepoMergeSettings {
    #[serde(rename = "nameWithOwner")]
    name_with_owner: String,
    #[serde(default, rename = "viewerDefaultMergeMethod")]
    viewer_default_merge_method: Option<String>,
    #[serde(default, rename = "mergeCommitAllowed")]
    merge_commit_allowed: bool,
    #[serde(default, rename = "rebaseMergeAllowed")]
    rebase_merge_allowed: bool,
    #[serde(default, rename = "squashMergeAllowed")]
    squash_merge_allowed: bool,
}

#[derive(serde::Deserialize)]
struct GithubMergeResponse {
    merged: bool,
    #[serde(default)]
    message: Option<String>,
}

fn preferred_merge_method(settings: &GithubRepoMergeSettings) -> Option<GithubMergeMethod> {
    let allowed = |method| match method {
        GithubMergeMethod::Merge => settings.merge_commit_allowed,
        GithubMergeMethod::Rebase => settings.rebase_merge_allowed,
        GithubMergeMethod::Squash => settings.squash_merge_allowed,
    };
    let viewer_default = match settings.viewer_default_merge_method.as_deref() {
        Some("MERGE") => Some(GithubMergeMethod::Merge),
        Some("REBASE") => Some(GithubMergeMethod::Rebase),
        Some("SQUASH") => Some(GithubMergeMethod::Squash),
        _ => None,
    };
    viewer_default
        .filter(|method| allowed(*method))
        .or_else(|| {
            [
                GithubMergeMethod::Squash,
                GithubMergeMethod::Merge,
                GithubMergeMethod::Rebase,
            ]
            .into_iter()
            .find(|method| allowed(*method))
        })
}

pub(super) fn repository_identity_and_merge_method(
    repo: &Path,
) -> anyhow::Result<(String, GithubMergeMethod)> {
    let settings_output = gh_output(gh_command_for_repo(repo)?
        .args([
            "repo",
            "view",
            "--json",
            "nameWithOwner,viewerDefaultMergeMethod,mergeCommitAllowed,rebaseMergeAllowed,squashMergeAllowed",
        ])
        .current_dir(repo)
        .env("GH_PROMPT_DISABLED", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
    )?;
    if !settings_output.status.success() {
        let stderr = String::from_utf8_lossy(&settings_output.stderr)
            .trim()
            .to_string();
        anyhow::bail!(if stderr.is_empty() {
            "Could not read the repository's allowed merge methods".to_string()
        } else {
            stderr
        });
    }
    let settings = serde_json::from_slice::<GithubRepoMergeSettings>(&settings_output.stdout)?;
    let method = preferred_merge_method(&settings).ok_or_else(|| {
        anyhow::anyhow!("This repository has no allowed pull request merge method")
    })?;
    Ok((settings.name_with_owner, method))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MergePullRequestOutcome {
    pub(crate) number: u64,
    pub(crate) branch: String,
    pub(crate) base_branch: String,
    head_sha: String,
    repository: String,
    github_branch_deleted: bool,
    branch_cleanup_error: Option<String>,
}

impl MergePullRequestOutcome {
    pub(crate) fn message(&self) -> String {
        let merged = format!(
            "Merged pull request #{} into {}",
            self.number, self.base_branch
        );
        if let Some(error) = &self.branch_cleanup_error {
            format!("{merged}, but the GitHub branch could not be deleted: {error}. Your local branch is kept.")
        } else if self.github_branch_deleted {
            format!(
                "{merged} and deleted {} on GitHub. Your local branch is kept.",
                self.branch
            )
        } else {
            merged
        }
    }

    pub(crate) fn notification(&self) -> Notification {
        if self.branch_cleanup_error.is_some() {
            Notification::warning(self.message())
        } else {
            Notification::success(self.message())
        }
    }
}

/// Branch cleanup is independent of merge success: its failure must never report
/// an already merged PR as failed or invite the user to merge it again.
pub(crate) fn merge_pull_request_with_branch_cleanup(
    repo: &Path,
    branch: &str,
    expected_base_branch: Option<&str>,
    expected_head_sha: Option<&str>,
    delete_branch: bool,
) -> anyhow::Result<MergePullRequestOutcome> {
    finish_merge_with_branch_cleanup(
        merge_pull_request_with_gh(repo, branch, expected_base_branch, expected_head_sha),
        delete_branch,
        |outcome| {
            delete_merged_github_branch(outcome, |method, endpoint| {
                github_branch_cleanup_api(repo, method, endpoint)
            })
        },
    )
}

fn finish_merge_with_branch_cleanup(
    result: anyhow::Result<MergePullRequestOutcome>,
    delete_branch: bool,
    cleanup: impl FnOnce(&MergePullRequestOutcome) -> anyhow::Result<()>,
) -> anyhow::Result<MergePullRequestOutcome> {
    let mut outcome = result?;
    if delete_branch {
        match cleanup(&outcome) {
            Ok(()) => outcome.github_branch_deleted = true,
            Err(error) => outcome.branch_cleanup_error = Some(format!("{error:#}")),
        }
    }
    Ok(outcome)
}

/// API-only cleanup: no Git checkout, branch deletion, pruning, or config edits.
fn github_branch_cleanup_api(
    repo: &Path,
    method: &str,
    endpoint: &str,
) -> anyhow::Result<Option<serde_json::Value>> {
    let output = gh_output(
        gh_command_for_repo(repo)?
            .args(["api", "--include", "--method", method, endpoint])
            .current_dir(repo)
            .env("GH_PROMPT_DISABLED", "1")
            .env("GIT_TERMINAL_PROMPT", "0"),
    )?;
    parse_github_branch_cleanup_response(&output)
}

fn parse_github_branch_cleanup_response(
    output: &std::process::Output,
) -> anyhow::Result<Option<serde_json::Value>> {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let status = stdout
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|status| status.parse::<u16>().ok());
    if status == Some(404) {
        return Ok(None);
    }
    if !output.status.success() || !status.is_some_and(|status| (200..300).contains(&status)) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("GitHub branch cleanup failed: {}", stderr.trim());
    }
    let body = stdout
        .split_once("\r\n\r\n")
        .or_else(|| stdout.split_once("\n\n"))
        .map(|(_, body)| body.trim())
        .ok_or_else(|| anyhow::anyhow!("GitHub did not return a valid API response"))?;
    Ok(Some(if body.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_str(body)?
    }))
}

fn delete_merged_github_branch(
    outcome: &MergePullRequestOutcome,
    mut api: impl FnMut(&str, &str) -> anyhow::Result<Option<serde_json::Value>>,
) -> anyhow::Result<()> {
    let pr_endpoint = format!("repos/{}/pulls/{}", outcome.repository, outcome.number);
    let pr = api("GET", &pr_endpoint)?
        .ok_or_else(|| anyhow::anyhow!("The merged pull request is unavailable"))?;
    let head = &pr["head"];
    if pr["merged"].as_bool() != Some(true)
        || head["ref"].as_str() != Some(outcome.branch.as_str())
        || head["sha"].as_str() != Some(outcome.head_sha.as_str())
    {
        anyhow::bail!("The pull request source changed; its GitHub branch was kept");
    }
    // A fork PR must target its head repository, not a same-named base branch.
    let source_repo = head["repo"]["full_name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("The source repository is unavailable"))?;
    let repo_endpoint = format!("repos/{source_repo}");
    let repository = api("GET", &repo_endpoint)?
        .ok_or_else(|| anyhow::anyhow!("The source repository is inaccessible"))?;
    if repository["default_branch"].as_str() == Some(outcome.branch.as_str()) {
        anyhow::bail!("The source is the repository's default branch; it was kept");
    }
    let encoded_branch = url_encode(&outcome.branch);
    let ref_endpoint = format!("repos/{source_repo}/git/ref/heads/{encoded_branch}");
    let Some(reference) = api("GET", &ref_endpoint)? else {
        // GitHub's automatic branch deletion may already have removed it.
        return Ok(());
    };
    if reference["ref"].as_str() != Some(format!("refs/heads/{}", outcome.branch).as_str())
        || reference["object"]["sha"].as_str() != Some(outcome.head_sha.as_str())
    {
        anyhow::bail!("The GitHub branch has new commits since the merge; it was kept");
    }
    let delete_endpoint = format!("repos/{source_repo}/git/refs/heads/{encoded_branch}");
    api("DELETE", &delete_endpoint)?;
    Ok(())
}

/// Immediately merges an open PR using the repository's preferred allowed
/// method. This uses GitHub's direct merge endpoint so a protected branch can
/// reject the attempt without `gh pr merge` silently enabling auto-merge or a
/// merge queue. It never bypasses protections or deletes the source branch.
pub(crate) fn merge_pull_request_with_gh(
    repo: &Path,
    branch: &str,
    expected_base_branch: Option<&str>,
    expected_head_sha: Option<&str>,
) -> anyhow::Result<MergePullRequestOutcome> {
    let output = gh_output(gh_command_for_repo(repo)?
        .args([
            "pr",
            "view",
            branch,
            "--json",
            "number,title,url,state,isDraft,headRefName,baseRefName,headRefOid,mergeStateStatus,reviewDecision,statusCheckRollup,autoMergeRequest",
        ])
        .current_dir(repo)
        .env("GH_PROMPT_DISABLED", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
    )?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        anyhow::bail!(if stderr.is_empty() {
            format!("No pull request was found for {branch}")
        } else {
            stderr
        });
    }
    let pr = branch_pull_request_from_github(
        serde_json::from_slice::<GithubPullRequest>(&output.stdout)?,
        branch,
    );
    if !pr.state.eq_ignore_ascii_case("OPEN") {
        anyhow::bail!(
            "Pull request #{} is already {}",
            pr.number,
            pr.state.to_ascii_lowercase()
        );
    }
    if pr.is_draft {
        anyhow::bail!("Pull request #{} is still a draft", pr.number);
    }
    if let Some(expected) = expected_base_branch
        .map(str::trim)
        .filter(|base| !base.is_empty())
    {
        if pr.base_branch != expected {
            anyhow::bail!(
                "Pull request #{} now targets {}, not {}",
                pr.number,
                pr.base_branch,
                expected
            );
        }
    }
    let head_oid = pr
        .head_oid
        .as_deref()
        .filter(|oid| !oid.is_empty())
        .ok_or_else(|| anyhow::anyhow!("GitHub did not return the PR head commit"))?;
    if let Some(expected) = expected_head_sha.filter(|sha| !sha.is_empty()) {
        if head_oid != expected {
            anyhow::bail!(
                "Pull request #{} source changed from {} to {}; refresh before merging",
                pr.number,
                expected,
                head_oid
            );
        }
    }

    let (name_with_owner, method) = repository_identity_and_merge_method(repo)?;

    let endpoint = format!("repos/{name_with_owner}/pulls/{}/merge", pr.number);
    let sha_field = format!("sha={head_oid}");
    let method_field = format!("merge_method={}", method.api_value());
    let merge_output = gh_output(
        gh_command_for_repo(repo)?
            .args(["api", "--method", "PUT"])
            .arg(endpoint)
            .args(["-f", &sha_field, "-f", &method_field])
            .current_dir(repo)
            .env("GH_PROMPT_DISABLED", "1")
            .env("GIT_TERMINAL_PROMPT", "0"),
    )?;
    if !merge_output.status.success() {
        let stderr = String::from_utf8_lossy(&merge_output.stderr)
            .trim()
            .to_string();
        anyhow::bail!(if stderr.is_empty() {
            format!("GitHub did not allow pull request #{} to merge", pr.number)
        } else {
            stderr
        });
    }
    let response = serde_json::from_slice::<GithubMergeResponse>(&merge_output.stdout)?;
    if !response.merged {
        anyhow::bail!(response.message.unwrap_or_else(|| format!(
            "GitHub did not allow pull request #{} to merge",
            pr.number
        )));
    }

    Ok(MergePullRequestOutcome {
        head_sha: head_oid.to_string(),
        repository: name_with_owner,
        github_branch_deleted: false,
        branch_cleanup_error: None,
        number: pr.number,
        branch: pr.branch,
        base_branch: pr.base_branch,
    })
}

impl GitPanel {
    pub(super) fn confirm_merge_pull_request(
        &mut self,
        git: Entity<GitState>,
        pr: BranchPullRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let title = format!("Merge pull request #{}?", pr.number);
        let panel = cx.entity();
        let expected_head_sha = pr.head_oid.clone();
        ConfirmDialog::new(
            title,
            "Choro will attempt the merge now using the repository's preferred allowed method. Required reviews, checks, and branch protections still apply.",
        )
        .tone(crate::ui::confirm::ConfirmTone::Primary)
        .icon(IconName::GitHub)
        .branch_route(pr.branch.clone(), pr.base_branch.clone())
        .checkbox(
            "Delete GitHub branch after merge",
            "Delete the source branch on GitHub only. Your local branch, files, and history are kept.",
        )
        .confirm_label("Merge PR")
        .confirm_id("confirm-merge-branch-pr")
        .on_confirm_with_checkbox(move |delete_branch, window, cx| {
            let window_handle = window.window_handle();
            let repo = git.read(cx).repo_path.clone();
            let branch = pr.branch.clone();
            let base_branch = pr.base_branch.clone();
            let git_for_task = git.clone();
            let git_for_result = git.clone();
            let pr_number = pr.number;
            let expected_head_sha = expected_head_sha.clone();
            panel.update(cx, |_panel, cx| {
                if git_for_task.read(cx).is_busy {
                    window.push_notification(
                        Notification::info("Finish the current Git operation before merging"),
                        cx,
                    );
                    return;
                }
                git_for_task.update(cx, |git, cx| {
                    git.is_busy = true;
                    git.last_error = None;
                    git.last_error_from_refresh = false;
                    git.last_message = Some(format!("Merging pull request #{pr_number}…"));
                    cx.notify();
                });

                cx.spawn(async move |this, cx| {
                    let result = cx
                        .background_executor()
                        .spawn(async move {
                            merge_pull_request_with_branch_cleanup(
                                &repo,
                                &branch,
                                Some(&base_branch),
                                expected_head_sha.as_deref(),
                                delete_branch,
                            )
                        })
                        .await;
                    git_for_result
                        .update(cx, |git, cx| {
                            git.is_busy = false;
                            match &result {
                                Ok(outcome) => {
                                    git.last_message = Some(outcome.message());
                                    git.last_error = None;
                                }
                                Err(error) => {
                                    git.last_message = None;
                                    git.last_error = Some(format!("{error:#}"));
                                    git.last_error_from_refresh = false;
                                }
                            }
                            git.refresh(cx);
                        })
                        .ok();
                    this.update(cx, |panel, cx| {
                        panel.branch_pr_checked_at = None;
                        panel.repo_prs_checked_at = None;
                        if let (Some(cached), Ok(outcome)) = (&mut panel.branch_pr, &result) {
                            if cached.number == outcome.number {
                                cached.state = "MERGED".into();
                            }
                        }
                        cx.notify();
                    })
                    .ok();
                    let notification = match &result {
                        Ok(outcome) => outcome.notification(),
                        Err(error) => Notification::error(format!("{error:#}")),
                    };
                    window_handle
                        .update(cx, |_, window, cx| {
                            window.push_notification(notification, cx)
                        })
                        .ok();
                })
                .detach();
            });
        })
        .open(window, cx);
    }
}

pub(crate) fn open_url(url: &str) {
    let mut command = Command::new("open");
    if is_pocketcomet_deep_link(url) {
        let (selector, application) =
            pocketcomet_open_target_for(Path::new(POCKETCOMET_APPLICATION_PATH).is_dir());
        command.arg(selector).arg(application);
    }
    if let Err(error) = command.arg("--").arg(url).spawn() {
        eprintln!("open url failed: {error}");
    }
}

const POCKETCOMET_APPLICATION_PATH: &str = "/Applications/PocketComet.app";
const POCKETCOMET_BUNDLE_ID: &str = "com.futurepicnic.dailybob";

fn pocketcomet_open_target_for(installed_application_exists: bool) -> (&'static str, &'static str) {
    if installed_application_exists {
        // DailyBob.app and PocketComet.app can share the legacy bundle id.
        // Select the renamed production app by path so Launch Services cannot
        // send the deep link to the obsolete DailyBob build.
        ("-a", POCKETCOMET_APPLICATION_PATH)
    } else {
        // Development builds may only be registered with Launch Services.
        ("-b", POCKETCOMET_BUNDLE_ID)
    }
}

fn is_pocketcomet_deep_link(url: &str) -> bool {
    url.starts_with("pocketcomet://") || url.starts_with("dailybob://")
}

pub(crate) fn pull_request_url_with_text(url: &str, pull_request: &GeneratedPullRequest) -> String {
    format!(
        "{url}&title={}&body={}",
        url_encode(&pull_request.title),
        url_encode(&pull_request.body)
    )
}

#[cfg(test)]
mod merge_tests {
    use super::*;

    fn merged_outcome() -> MergePullRequestOutcome {
        MergePullRequestOutcome {
            number: 42,
            branch: "feature/old-data".into(),
            base_branch: "main".into(),
            head_sha: "abc123".into(),
            repository: "owner/app".into(),
            github_branch_deleted: false,
            branch_cleanup_error: None,
        }
    }

    fn merged_pr() -> serde_json::Value {
        serde_json::json!({
            "merged": true,
            "head": {
                "ref": "feature/old-data", "sha": "abc123",
                "repo": { "full_name": "contributor/fork" }
            }
        })
    }

    #[test]
    fn github_cleanup_targets_only_the_exact_fork_ref() {
        let mut calls = Vec::new();
        delete_merged_github_branch(&merged_outcome(), |method, endpoint| {
            calls.push((method.to_string(), endpoint.to_string()));
            Ok(Some(match calls.len() {
                1 => merged_pr(),
                2 => serde_json::json!({ "default_branch": "main" }),
                3 => serde_json::json!({
                    "ref": "refs/heads/feature/old-data", "object": { "sha": "abc123" }
                }),
                4 => serde_json::Value::Null,
                _ => panic!("unexpected API request"),
            }))
        })
        .unwrap();
        assert_eq!(
            calls,
            vec![
                ("GET".into(), "repos/owner/app/pulls/42".into()),
                ("GET".into(), "repos/contributor/fork".into()),
                (
                    "GET".into(),
                    "repos/contributor/fork/git/ref/heads/feature%2Fold-data".into()
                ),
                (
                    "DELETE".into(),
                    "repos/contributor/fork/git/refs/heads/feature%2Fold-data".into()
                ),
            ]
        );
    }

    #[test]
    fn github_cleanup_keeps_a_branch_with_new_commits() {
        let mut calls = 0;
        let result = delete_merged_github_branch(&merged_outcome(), |method, _| {
            assert_eq!(method, "GET", "a changed branch must never be deleted");
            calls += 1;
            Ok(Some(match calls {
                1 => merged_pr(),
                2 => serde_json::json!({ "default_branch": "main" }),
                _ => serde_json::json!({
                    "ref": "refs/heads/feature/old-data", "object": { "sha": "new-work" }
                }),
            }))
        });
        assert!(result.unwrap_err().to_string().contains("new commits"));
    }

    #[test]
    fn github_cleanup_accepts_already_deleted_branches() {
        let mut calls = 0;
        delete_merged_github_branch(&merged_outcome(), |method, _| {
            assert_eq!(method, "GET");
            calls += 1;
            Ok(match calls {
                1 => Some(merged_pr()),
                2 => Some(serde_json::json!({ "default_branch": "main" })),
                _ => None,
            })
        })
        .unwrap();
        assert_eq!(calls, 3);
    }

    #[test]
    fn github_cleanup_refuses_an_unmerged_pr_or_a_default_branch() {
        let mut pr = merged_pr();
        pr["merged"] = false.into();
        assert!(delete_merged_github_branch(&merged_outcome(), |method, _| {
            assert_eq!(method, "GET");
            Ok(Some(pr.clone()))
        })
        .is_err());
        let mut calls = 0;
        assert!(delete_merged_github_branch(&merged_outcome(), |method, _| {
            assert_eq!(method, "GET");
            calls += 1;
            Ok(Some(if calls == 1 {
                merged_pr()
            } else {
                serde_json::json!({ "default_branch": "feature/old-data" })
            }))
        })
        .is_err());
        assert_eq!(calls, 2);
    }

    #[test]
    fn unchecked_or_failed_merges_never_run_branch_cleanup() {
        let outcome = finish_merge_with_branch_cleanup(Ok(merged_outcome()), false, |_| {
            panic!("unchecked cleanup must not run")
        })
        .unwrap();
        assert!(!outcome.github_branch_deleted);
        assert!(
            finish_merge_with_branch_cleanup(Err(anyhow::anyhow!("blocked")), true, |_| {
                panic!("failed merges must not run cleanup")
            })
            .is_err()
        );
    }

    #[test]
    fn deletion_failure_preserves_merge_success_and_explains_local_retention() {
        let outcome = finish_merge_with_branch_cleanup(Ok(merged_outcome()), true, |_| {
            anyhow::bail!("permission denied")
        })
        .unwrap();
        assert!(!outcome.github_branch_deleted);
        assert_eq!(
            outcome.branch_cleanup_error.as_deref(),
            Some("permission denied")
        );
        assert!(outcome.message().contains("Merged pull request #42"));
        assert!(outcome.message().contains("Your local branch is kept"));
    }

    #[test]
    fn cleanup_parses_github_headers_json_empty_delete_and_not_found() {
        use std::os::unix::process::ExitStatusExt;

        let output = |code, stdout: &str| std::process::Output {
            status: std::process::ExitStatus::from_raw(code << 8),
            stdout: stdout.as_bytes().to_vec(),
            stderr: b"GitHub API request failed".to_vec(),
        };
        let response = output(
            0,
            "HTTP/2.0 200 OK\r\nContent-Type: application/json\r\n\r\n{\"merged\":true}\n",
        );
        assert_eq!(
            parse_github_branch_cleanup_response(&response).unwrap(),
            Some(serde_json::json!({"merged": true}))
        );
        let response = output(0, "HTTP/2.0 204 No Content\r\nX-Github: test\r\n\r\n");
        assert_eq!(
            parse_github_branch_cleanup_response(&response).unwrap(),
            Some(serde_json::Value::Null)
        );
        let response = output(
            1,
            "HTTP/2.0 404 Not Found\r\n\r\n{\"message\":\"Not Found\"}",
        );
        assert_eq!(
            parse_github_branch_cleanup_response(&response).unwrap(),
            None
        );
        let response = output(1, "HTTP/2.0 403 Forbidden\r\n\r\n{}");
        assert!(parse_github_branch_cleanup_response(&response).is_err());
    }

    #[test]
    fn recognizes_only_pocketcomet_deep_links_for_app_targeting() {
        assert!(is_pocketcomet_deep_link("pocketcomet://document/123"));
        assert!(is_pocketcomet_deep_link("dailybob://task/123"));
        assert!(!is_pocketcomet_deep_link("https://example.com"));
    }

    #[test]
    fn pocketcomet_open_target_prefers_the_renamed_installed_app() {
        assert_eq!(
            pocketcomet_open_target_for(true),
            ("-a", "/Applications/PocketComet.app")
        );
        assert_eq!(
            pocketcomet_open_target_for(false),
            ("-b", "com.futurepicnic.dailybob")
        );
    }

    #[test]
    fn preferred_merge_method_uses_allowed_viewer_default() {
        let settings = GithubRepoMergeSettings {
            name_with_owner: "owner/repo".into(),
            viewer_default_merge_method: Some("REBASE".into()),
            merge_commit_allowed: true,
            rebase_merge_allowed: true,
            squash_merge_allowed: true,
        };
        assert_eq!(
            preferred_merge_method(&settings),
            Some(GithubMergeMethod::Rebase)
        );
    }

    #[test]
    fn preferred_merge_method_falls_back_to_an_allowed_method() {
        let settings = GithubRepoMergeSettings {
            name_with_owner: "owner/repo".into(),
            viewer_default_merge_method: Some("MERGE".into()),
            merge_commit_allowed: false,
            rebase_merge_allowed: true,
            squash_merge_allowed: true,
        };
        assert_eq!(
            preferred_merge_method(&settings),
            Some(GithubMergeMethod::Squash)
        );
    }
}
