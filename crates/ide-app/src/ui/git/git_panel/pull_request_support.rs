use super::*;

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
    let output = gh_command()
        .ok()?
        .args(["pr", "view", branch, "--json", "url", "--jq", ".url"])
        .current_dir(repo)
        .env("GH_PROMPT_DISABLED", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !url.is_empty() {
        return Some(url);
    }

    let output = gh_command()
        .ok()?
        .args([
            "pr", "list", "--head", branch, "--state", "all", "--limit", "1", "--json", "url",
            "--jq", ".[0].url",
        ])
        .current_dir(repo)
        .env("GH_PROMPT_DISABLED", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
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
) -> Option<String> {
    let output = gh_command()
        .ok()?
        .args([
            "pr",
            "list",
            "--head",
            branch,
            "--base",
            base_branch,
            "--state",
            "all",
            "--limit",
            "1",
            "--json",
            "url",
            "--jq",
            ".[0].url",
        ])
        .current_dir(repo)
        .env("GH_PROMPT_DISABLED", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!url.is_empty() && url != "null").then_some(url)
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
    if let Some(url) = existing_pull_request_url_for_base(repo, branch, base_branch) {
        return Ok(url);
    }

    let output = gh_command()?
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
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if let Some(url) = stdout
            .split_whitespace()
            .find(|part| part.starts_with("https://github.com/"))
        {
            return Ok(url.trim().to_string());
        }
        if let Some(url) = existing_pull_request_url_for_base(repo, branch, base_branch) {
            return Ok(url);
        }
        anyhow::bail!("GitHub CLI created the pull request but did not return a URL");
    }

    if let Some(url) = existing_pull_request_url_for_base(repo, branch, base_branch) {
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
        number: pr.number,
        title: pr.title,
        url: pr.url,
        state: pr.state,
        is_draft: pr.is_draft,
        merge_state_status: pr.merge_state_status,
        review_decision: pr.review_decision,
        check_state: pull_request_check_state(&pr.status_check_rollup),
    }
}

pub(crate) fn branch_pull_request(repo: &Path, branch: &str) -> Option<BranchPullRequest> {
    if branch == default_remote_branch(repo) || matches!(branch, "main" | "master") {
        return None;
    }

    let view_output = gh_command()
        .ok()?
        .args([
            "pr",
            "view",
            branch,
            "--json",
            "number,title,url,state,isDraft,headRefName,mergeStateStatus,reviewDecision,statusCheckRollup",
        ])
        .current_dir(repo)
        .env("GH_PROMPT_DISABLED", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .ok()?;
    if view_output.status.success() {
        if let Ok(pr) = serde_json::from_slice::<GithubPullRequest>(&view_output.stdout) {
            return Some(branch_pull_request_from_github(pr, branch));
        }
    }

    let output = gh_command()
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
            "number,title,url,state,isDraft,mergeStateStatus,reviewDecision,statusCheckRollup",
        ])
        .current_dir(repo)
        .env("GH_PROMPT_DISABLED", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
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
    let output = gh_command()?
        .args([
            "pr",
            "list",
            "--state",
            "all",
            "--limit",
            "30",
            "--json",
            "number,title,url,state,isDraft,headRefName,mergeStateStatus,reviewDecision,statusCheckRollup",
        ])
        .current_dir(repo)
        .env("GH_PROMPT_DISABLED", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()?;
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

pub(crate) fn open_url(url: &str) {
    if let Err(error) = Command::new("open").arg("--").arg(url).spawn() {
        eprintln!("open pull request url failed: {error}");
    }
}

pub(crate) fn pull_request_url_with_text(url: &str, pull_request: &GeneratedPullRequest) -> String {
    format!(
        "{url}&title={}&body={}",
        url_encode(&pull_request.title),
        url_encode(&pull_request.body)
    )
}
