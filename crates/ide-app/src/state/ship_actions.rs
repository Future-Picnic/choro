//! App-owned GitHub watcher; external automation never touches chat execution.
use super::{agent_chat::ShipResult, AgentRecords, Workspace};
use anyhow::{Context as _, Result};
use gpui::{App, AppContext, Context, Entity, Global};
use ide_core::{local_store::LocalStore, ship_actions::*, AgentRecord};
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

#[derive(Clone)]
pub(crate) struct ShipActionsHandle(pub Entity<ShipActionsState>);
impl Global for ShipActionsHandle {}

pub(crate) struct ShipActionsState {
    pub watches: HashMap<Uuid, ShipActionsWatch>,
    pub error: Option<String>,
    workspace: Entity<Workspace>,
    agents: Entity<AgentRecords>,
    loaded: bool,
    in_flight: HashSet<Uuid>,
    epochs: HashMap<Uuid, u64>,
    dirty: HashSet<Uuid>,
    saving: HashSet<Uuid>,
    pending_registrations: Vec<(AgentRecord, ShipResult, PathBuf, String)>,
}

pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

impl ShipActionsState {
    pub fn is_loaded(&self) -> bool {
        self.loaded
    }
    pub fn start(
        workspace: Entity<Workspace>,
        agents: Entity<AgentRecords>,
        cx: &mut App,
    ) -> Entity<Self> {
        if let Some(handle) = cx.try_global::<ShipActionsHandle>() {
            return handle.0.clone();
        }
        let state = cx.new(|_| Self {
            watches: HashMap::new(),
            error: None,
            workspace,
            agents,
            loaded: false,
            in_flight: HashSet::new(),
            epochs: HashMap::new(),
            dirty: HashSet::new(),
            saving: HashSet::new(),
            pending_registrations: vec![],
        });
        cx.set_global(ShipActionsHandle(state.clone()));
        state.update(cx, |_, cx| {
            cx.spawn(async move |this, cx| loop {
                let loaded = this.update(cx, |s, _| s.loaded).unwrap_or(true);
                if !loaded {
                    let result = cx
                        .background_executor()
                        .spawn(async { LocalStore::open_default()?.load_ship_actions_watches() })
                        .await;
                    let _ = this.update(cx, |s, cx| match result {
                        Ok(watches) => {
                            for mut w in watches {
                                if w.automatically_polled() {
                                    w.refresh_error =
                                        Some("Refreshing saved GitHub Actions…".into());
                                }
                                w.next_poll_at = now();
                                s.watches.entry(w.id).or_insert(w);
                            }
                            s.loaded = true;
                            s.error = None;
                            for (agent, ship, path, name) in
                                std::mem::take(&mut s.pending_registrations)
                            {
                                s.register(&agent, &ship, path, name, cx);
                            }
                            cx.notify();
                        }
                        Err(e) => {
                            s.error = Some(format!("Unable to restore Actions: {e:#}"));
                            cx.notify();
                        }
                    });
                }
                if this.update(cx, |s, cx| s.tick(cx)).is_err() {
                    break;
                }
                cx.background_executor()
                    .timer(Duration::from_secs(if loaded { 1 } else { 5 }))
                    .await;
            })
            .detach();
        });
        state
    }

    pub fn watch_for_ship(&self, agent: Uuid, ship: &str) -> Option<&ShipActionsWatch> {
        self.watches.values().find(|w| {
            w.origins
                .iter()
                .any(|o| o.agent_id == agent && o.ship_id == ship)
        })
    }

    /// A GitHub URL captures repository identity before source-branch cleanup.
    pub fn register(
        &mut self,
        agent: &AgentRecord,
        ship: &ShipResult,
        path: PathBuf,
        project_name: String,
        cx: &mut Context<Self>,
    ) {
        if !self.loaded {
            self.pending_registrations
                .push((agent.clone(), ship.clone(), path, project_name));
            return;
        }
        let Some(url) = ship.pr_url.as_deref() else {
            return;
        };
        let Some((host, repository, number)) = parse_pr_url(url) else {
            return;
        };
        let existing = self
            .watches
            .values()
            .find(|w| {
                w.project_id == agent.project_id
                    && w.host == host
                    && w.repository.eq_ignore_ascii_case(&repository)
                    && w.pr_number == number
            })
            .map(|w| w.id);
        let id = existing.unwrap_or_else(Uuid::new_v4);
        let time = now();
        let watch = self.watches.entry(id).or_insert_with(|| ShipActionsWatch {
            id,
            revision: 0,
            project_id: agent.project_id,
            project_name,
            host,
            repository,
            repository_path: path,
            pr_number: number,
            pr_url: url.into(),
            base_branch: ship.pr_base_branch.clone().unwrap_or_default(),
            merge_sha: None,
            merged_at: None,
            monitoring_started_at: None,
            phase: TrackingPhase::AwaitingMerge,
            origins: vec![],
            runs: vec![],
            discovery_seconds: 0,
            last_checked_at: None,
            refresh_error: None,
            error_count: 0,
            next_poll_at: time,
            stable_terminal_scans: 0,
            delivered_revision: None,
            acknowledged_revision: None,
            created_at: time,
            updated_at: time,
        });
        if watch
            .origins
            .iter()
            .any(|o| o.agent_id == agent.id && o.ship_id == ship.id)
        {
            return;
        }
        watch.origins.push(ShipActionsOrigin {
            agent_id: agent.id,
            ship_id: ship.id.clone(),
            agent_title: agent.title.clone(),
            created_at: ship.created_at,
        });
        self.changed(id, cx);
    }

    pub fn refresh(&mut self, id: Uuid, cx: &mut Context<Self>) {
        if let Some(w) = self.watches.get_mut(&id) {
            w.resume(now());
        }
        *self.epochs.entry(id).or_default() += 1;
        self.changed(id, cx);
    }
    pub fn stop_tracking(&mut self, id: Uuid, cx: &mut Context<Self>) {
        if let Some(w) = self.watches.get_mut(&id) {
            w.phase = TrackingPhase::Stopped;
            crate::notifications::remove_ship_actions_notice(w);
        }
        *self.epochs.entry(id).or_default() += 1;
        self.changed(id, cx);
    }
    pub fn acknowledge(&mut self, id: Uuid, cx: &mut Context<Self>) {
        if let Some(w) = self.watches.get_mut(&id) {
            w.acknowledged_revision = w.notice_revision();
            crate::notifications::remove_ship_actions_notice(w);
        }
        self.changed(id, cx);
    }
    fn changed(&mut self, id: Uuid, cx: &mut Context<Self>) {
        if let Some(w) = self.watches.get_mut(&id) {
            w.revision += 1;
            w.updated_at = now();
            self.dirty.insert(id);
        }
        cx.notify();
    }
    fn tick(&mut self, cx: &mut Context<Self>) {
        if !self.loaded {
            return;
        }
        // Only projects and origins still in the workspace may issue requests.
        let projects: HashSet<_> = self
            .workspace
            .read(cx)
            .projects
            .iter()
            .map(|p| p.id)
            .collect();
        let live_agents: HashSet<_> = self.agents.read(cx).iter_records().map(|a| a.id).collect();
        let mut removed_origin = false;
        for watch in self.watches.values_mut() {
            let before = watch.origins.len();
            if watch
                .origins
                .iter()
                .any(|origin| !live_agents.contains(&origin.agent_id))
            {
                crate::notifications::remove_ship_actions_notice(watch);
            }
            watch
                .origins
                .retain(|origin| live_agents.contains(&origin.agent_id));
            if before != watch.origins.len() {
                removed_origin = true;
            }
        }
        if removed_origin {
            cx.notify();
        }
        self.dirty.retain(|id| {
            self.watches
                .get(id)
                .is_some_and(|w| projects.contains(&w.project_id))
        });
        let mut due = self
            .watches
            .values()
            .filter(|w| {
                projects.contains(&w.project_id)
                    && !w.origins.is_empty()
                    && w.automatically_polled()
                    && w.next_poll_at <= now()
                    && !self.in_flight.contains(&w.id)
            })
            .cloned()
            .collect::<Vec<_>>();
        due.sort_by_key(|w| w.next_poll_at);
        for watch in due
            .into_iter()
            .take(4usize.saturating_sub(self.in_flight.len()))
        {
            let id = watch.id;
            let epoch = *self.epochs.entry(id).or_default();
            self.in_flight.insert(id);
            cx.spawn(async move |this, cx| {
                let result = cx
                    .background_executor()
                    .spawn(async move { poll(watch) })
                    .await;
                let _ = this.update(cx, |s, cx| {
                    s.in_flight.remove(&id);
                    if s.epochs.get(&id).copied().unwrap_or_default() != epoch {
                        return;
                    }
                    let Some(current) = s.watches.get_mut(&id) else {
                        return;
                    };
                    match result {
                        Ok(mut fresh) => {
                            fresh.origins = current.origins.clone();
                            fresh.revision = current.revision;
                            fresh.delivered_revision = current.delivered_revision.clone();
                            fresh.acknowledged_revision = current.acknowledged_revision.clone();
                            *current = fresh;
                            if current.notice_revision().is_none() {
                                crate::notifications::remove_ship_actions_notice(current);
                            }
                        }
                        Err(e) => {
                            current.refresh_error = Some(format!("{e:#}"));
                            current.error_count = current.error_count.saturating_add(1);
                            let retry = e
                                .downcast_ref::<GithubRequestError>()
                                .and_then(|e| e.retry_after)
                                .unwrap_or(
                                    30u64
                                        .saturating_mul(
                                            1u64 << current.error_count.min(4).saturating_sub(1),
                                        )
                                        .min(300),
                                );
                            current.next_poll_at = now() + retry.max(1);
                        }
                    }
                    s.changed(id, cx);
                });
            })
            .detach();
        }
        // One ordered writer per watch. Acknowledgments and registrations made
        // during a save stay dirty and are flushed after that save completes.
        let dirty = self
            .dirty
            .iter()
            .copied()
            .filter(|id| !self.saving.contains(id))
            .collect::<Vec<_>>();
        for id in dirty {
            let Some(watch) = self.watches.get(&id).cloned() else {
                continue;
            };
            self.saving.insert(id);
            let revision = watch.revision;
            let notice = watch.notice_revision().filter(|r| {
                watch.delivered_revision.as_ref() != Some(r)
                    && watch.acknowledged_revision.as_ref() != Some(r)
            });
            cx.spawn(async move |this, cx| {
                let mut saved = watch.clone();
                if notice.is_some() {
                    saved.delivered_revision = notice.clone();
                }
                let result = cx
                    .background_executor()
                    .spawn(
                        async move { LocalStore::open_default()?.save_ship_actions_watch(&saved) },
                    )
                    .await;
                let _ = this.update(cx, |s, cx| {
                    s.saving.remove(&id);
                    match result {
                        Ok(()) => {
                            if s.watches.get(&id).is_some_and(|w| w.revision == revision) {
                                s.dirty.remove(&id);
                            }
                            s.error = None;
                            // Delivery is at most once across recovery: the saved
                            // snapshot already includes this exact notice marker.
                            let preferences = s.workspace.read(cx).notifications;
                            let companion = s.workspace.read(cx).companion_enabled;
                            if let Some(w) = s.watches.get_mut(&id) {
                                if let Some(notice) = notice.filter(|r| {
                                    w.notice_revision().as_ref() == Some(r)
                                        && w.acknowledged_revision.as_ref() != Some(r)
                                }) {
                                    w.delivered_revision = Some(notice);
                                    crate::notifications::deliver_ship_actions_notice(
                                        w,
                                        preferences,
                                        companion,
                                    );
                                    if w.revision != revision {
                                        s.changed(id, cx);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            s.error = Some(format!("Unable to save Actions: {e:#}"));
                            cx.notify();
                        }
                    }
                });
            })
            .detach();
        }
    }
}

fn parse_pr_url(value: &str) -> Option<(String, String, u64)> {
    let url = url::Url::parse(value).ok()?;
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        return None;
    }
    let parts = url.path_segments()?.collect::<Vec<_>>();
    if parts.len() != 4
        || parts[2] != "pull"
        || parts[..2].iter().any(|p| {
            p.is_empty()
                || !p
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
        })
    {
        return None;
    }
    let number = parts[3].parse::<u64>().ok().filter(|number| *number > 0)?;
    Some((
        url.host_str()?.to_owned(),
        format!("{}/{}", parts[0], parts[1]),
        number,
    ))
}

#[derive(Debug)]
struct GithubRequestError {
    message: String,
    retry_after: Option<u64>,
}
impl std::fmt::Display for GithubRequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}
impl std::error::Error for GithubRequestError {}

fn response_parts(text: &str) -> Result<(&str, &str)> {
    if !text.starts_with("HTTP/") {
        return Ok(("", text));
    }
    text.split_once("\r\n\r\n")
        .or_else(|| text.split_once("\n\n"))
        .context("GitHub returned an incomplete HTTP response")
}

fn retry_after(headers: &str, time: u64) -> Option<u64> {
    let header = |name: &str| {
        headers
            .lines()
            .filter_map(|line| line.split_once(':'))
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.trim())
    };
    if let Some(value) = header("retry-after") {
        if let Ok(seconds) = value.parse::<u64>() {
            return Some(seconds.max(1));
        }
        if let Ok(date) = chrono::DateTime::parse_from_rfc2822(value) {
            return Some((date.timestamp().max(0) as u64).saturating_sub(time).max(1));
        }
    }
    if header("x-ratelimit-remaining") == Some("0") {
        header("x-ratelimit-reset")?
            .parse::<u64>()
            .ok()
            .map(|reset| reset.saturating_sub(time).max(1))
    } else {
        None
    }
}

fn request(w: &ShipActionsWatch, endpoint: &str) -> Result<serde_json::Value> {
    let mut cmd = crate::ui::git::git_panel::gh_command_for_repo(&w.repository_path)?;
    cmd.args(["api", "--hostname", &w.host, "--include", endpoint]);
    cmd.current_dir(&w.repository_path)
        .env("GH_PROMPT_DISABLED", "1")
        .env("GIT_TERMINAL_PROMPT", "0");
    let out = ide_core::process::output_with_timeout(&mut cmd, Duration::from_secs(60))?;
    let text = String::from_utf8_lossy(&out.stdout);
    if !out.status.success() {
        let retry_after = retry_after(
            response_parts(&text)
                .map(|(headers, _)| headers)
                .unwrap_or_default(),
            now(),
        );
        return Err(GithubRequestError {
            message: format!(
                "GitHub request failed: {}. Check connection and GitHub authentication.",
                String::from_utf8_lossy(&out.stderr).trim()
            ),
            retry_after,
        }
        .into());
    }
    // gh --include prefixes each page with HTTP headers; pagination is handled
    // explicitly below so responses remain ordinary JSON, never mixed pages.
    let (_, body) = response_parts(&text)?;
    Ok(serde_json::from_str(body)?)
}

fn timestamp(value: &str) -> Result<u64> {
    Ok(chrono::DateTime::parse_from_rfc3339(value)?
        .timestamp()
        .try_into()?)
}

#[derive(Deserialize)]
struct Pull {
    state: String,
    merged: bool,
    merged_at: Option<String>,
    merge_commit_sha: Option<String>,
    base: PullBase,
}
#[derive(Deserialize)]
struct PullBase {
    #[serde(rename = "ref")]
    branch: String,
}

fn poll(watch: ShipActionsWatch) -> Result<ShipActionsWatch> {
    poll_with(watch, now(), request)
}

fn poll_with(
    mut watch: ShipActionsWatch,
    time: u64,
    mut fetch: impl FnMut(&ShipActionsWatch, &str) -> Result<serde_json::Value>,
) -> Result<ShipActionsWatch> {
    if watch.merge_sha.is_none() {
        let pr: Pull = serde_json::from_value(fetch(
            &watch,
            &format!("repos/{}/pulls/{}", watch.repository, watch.pr_number),
        )?)?;
        watch.last_checked_at = Some(time);
        watch.refresh_error = None;
        watch.error_count = 0;
        if !pr.merged {
            watch.phase = if pr.state == "closed" {
                TrackingPhase::Closed
            } else {
                TrackingPhase::AwaitingMerge
            };
            watch.next_poll_at = time + 60;
            return Ok(watch);
        }
        watch.merge_sha = Some(
            pr.merge_commit_sha
                .context("Merged PR has no resulting commit yet")?,
        );
        watch.merged_at = Some(timestamp(
            &pr.merged_at.context("Merged PR has no merge time yet")?,
        )?);
        watch.monitoring_started_at = Some(time);
        watch.base_branch = pr.base.branch;
        watch.phase = TrackingPhase::Discovering;
    }
    if watch
        .monitoring_started_at
        .is_some_and(|start| time.saturating_sub(start) >= MAX_TRACKING_SECONDS)
    {
        watch.phase = TrackingPhase::Paused;
        return Ok(watch);
    }
    let mut runs = vec![];
    for page in 1..=100 {
        let endpoint = format!(
            "repos/{}/actions/runs?head_sha={}&per_page=100&page={page}",
            watch.repository,
            watch.merge_sha.as_deref().unwrap()
        );
        let response = fetch(&watch, &endpoint)?;
        let page_runs = response
            .get("workflow_runs")
            .and_then(|v| v.as_array())
            .context("GitHub returned no workflow run list")?;
        for value in page_runs {
            runs.push(parse_run(value.clone())?);
        }
        if page_runs.len() < 100 {
            break;
        }
        anyhow::ensure!(
            page < 100,
            "Too many workflow runs to verify completion safely"
        );
    }
    // Refresh known IDs as well: pagination or delayed list indexing must not
    // leave a run's terminal result or new attempt permanently hidden.
    for known in &watch.runs {
        if !runs.iter().any(|r| r.id == known.id) {
            runs.push(parse_run(fetch(
                &watch,
                &format!("repos/{}/actions/runs/{}", watch.repository, known.id),
            )?)?);
        }
    }
    watch.observe_runs(runs, time);
    Ok(watch)
}

fn parse_run(value: serde_json::Value) -> Result<GithubActionRun> {
    #[derive(Deserialize)]
    struct Run {
        id: u64,
        workflow_id: u64,
        name: Option<String>,
        event: String,
        head_sha: String,
        head_branch: Option<String>,
        #[serde(default = "one")]
        run_attempt: u64,
        status: String,
        conclusion: Option<String>,
        created_at: String,
        updated_at: String,
        run_started_at: Option<String>,
        html_url: String,
    }
    fn one() -> u64 {
        1
    }
    let r: Run = serde_json::from_value(value)?;
    Ok(GithubActionRun {
        id: r.id,
        workflow_id: r.workflow_id,
        name: r.name.unwrap_or_else(|| "GitHub workflow".into()),
        event: r.event,
        head_sha: r.head_sha,
        head_branch: r.head_branch.unwrap_or_default(),
        attempt: r.run_attempt,
        status: r.status,
        conclusion: r.conclusion,
        created_at: timestamp(&r.created_at)?,
        updated_at: timestamp(&r.updated_at)?,
        started_at: r.run_started_at.as_deref().map(timestamp).transpose()?,
        html_url: r.html_url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn github_headers_and_rate_limits_are_parsed_without_confusing_json() {
        assert_eq!(
            response_parts("HTTP/2.0 200 OK\r\nX-Test: yes\r\n\r\n{\"workflow_runs\":[]}")
                .unwrap()
                .1,
            "{\"workflow_runs\":[]}"
        );
        assert_eq!(
            response_parts("HTTP/2.0 200 OK\nX-Test: yes\n\n{}")
                .unwrap()
                .1,
            "{}"
        );
        assert_eq!(response_parts("{\n\n\"workflow_runs\":[]}").unwrap().0, "");
        assert!(response_parts("HTTP/2.0 200 OK\nX-Test: yes").is_err());
        assert_eq!(retry_after("Retry-After: 45", 100), Some(45));
        assert_eq!(
            retry_after("X-RateLimit-Remaining: 0\r\nX-RateLimit-Reset: 250", 100),
            Some(150)
        );
        assert_eq!(
            retry_after("X-RateLimit-Remaining: 100\r\nX-RateLimit-Reset: 250", 100),
            None
        );
    }
    #[test]
    fn pr_identity_is_explicit_and_safe() {
        assert_eq!(
            parse_pr_url("https://github.com/owner/repo/pull/17"),
            Some(("github.com".into(), "owner/repo".into(), 17))
        );
        for url in [
            "onboarding-demo://pr/1",
            "https://github.com/owner/repo/compare/17",
            "https://user@github.com/owner/repo/pull/17",
            "https://github.com/owner/repo/pull/17/extra",
        ] {
            assert!(parse_pr_url(url).is_none());
        }
    }
    #[test]
    fn github_status_and_attempt_survive_parsing() {
        let r=parse_run(serde_json::json!({"id":1,"workflow_id":2,"name":"Deploy","event":"push","head_sha":"merge","head_branch":"main","run_attempt":3,"status":"waiting","conclusion":null,"created_at":"2026-10-17T10:00:00Z","updated_at":"2026-10-17T10:01:00Z","run_started_at":null,"html_url":"https://github.com/o/r/actions/runs/1"})).unwrap();
        assert_eq!(r.attempt, 3);
        assert_eq!(r.outcome(), ActionsOutcome::Approval);
    }
    fn watch() -> ShipActionsWatch {
        ShipActionsWatch {
            id: Uuid::new_v4(),
            revision: 1,
            project_id: ide_core::ProjectId(Uuid::new_v4()),
            project_name: "Project".into(),
            host: "github.com".into(),
            repository: "o/r".into(),
            repository_path: "/tmp/repo".into(),
            pr_number: 7,
            pr_url: "https://github.com/o/r/pull/7".into(),
            base_branch: "main".into(),
            merge_sha: None,
            merged_at: None,
            monitoring_started_at: None,
            phase: TrackingPhase::AwaitingMerge,
            origins: vec![],
            runs: vec![],
            discovery_seconds: 0,
            last_checked_at: None,
            refresh_error: None,
            error_count: 0,
            next_poll_at: 0,
            stable_terminal_scans: 0,
            delivered_revision: None,
            acknowledged_revision: None,
            created_at: 0,
            updated_at: 0,
        }
    }
    fn run_json() -> serde_json::Value {
        serde_json::json!({"id":1,"workflow_id":2,"name":"Deploy","event":"push","head_sha":"merge","head_branch":"main","run_attempt":1,"status":"in_progress","conclusion":null,"created_at":"2026-10-17T10:00:00Z","updated_at":"2026-10-17T10:01:00Z","run_started_at":"2026-10-17T10:00:01Z","html_url":"https://github.com/o/r/actions/runs/1"})
    }
    #[test]
    fn external_merge_uses_result_sha_and_never_source_branch() {
        let time = timestamp("2026-10-17T10:00:00Z").unwrap();
        let mut endpoints = vec![];
        let w=poll_with(watch(),time,|_,endpoint| {
            endpoints.push(endpoint.to_string());
            if endpoint.ends_with("/pulls/7") { Ok(serde_json::json!({"state":"closed","merged":true,"merged_at":"2026-10-17T10:00:00Z","merge_commit_sha":"merge","base":{"ref":"main"}})) }
            else { Ok(serde_json::json!({"workflow_runs":[run_json()]})) }
        }).unwrap();
        assert_eq!(w.merge_sha.as_deref(), Some("merge"));
        assert_eq!(w.runs.len(), 1);
        assert_eq!(w.outcome(), ActionsOutcome::Running);
        assert!(endpoints[1].contains("head_sha=merge"));
    }
    #[test]
    fn closed_unmerged_pr_never_requests_actions() {
        let w=poll_with(watch(),200,|_,endpoint| {
            assert!(endpoint.ends_with("/pulls/7"));
            Ok(serde_json::json!({"state":"closed","merged":false,"merged_at":null,"merge_commit_sha":null,"base":{"ref":"main"}}))
        }).unwrap();
        assert_eq!(w.phase, TrackingPhase::Closed);
        assert!(w.runs.is_empty());
        assert!(w.notice_revision().is_none());
    }
    #[test]
    fn all_pages_are_checked_and_unrelated_runs_are_ignored() {
        let time = timestamp("2026-10-17T10:00:00Z").unwrap();
        let mut w = watch();
        w.merge_sha = Some("merge".into());
        w.merged_at = Some(time);
        w.monitoring_started_at = Some(time);
        let mut page_one = vec![];
        for id in 1..=100 {
            let mut r = run_json();
            r["id"] = id.into();
            r["head_sha"] = "unrelated".into();
            page_one.push(r);
        }
        let mut calls = 0;
        let w = poll_with(w, time, |_, endpoint| {
            calls += 1;
            Ok(if endpoint.ends_with("page=1") {
                serde_json::json!({"workflow_runs":page_one})
            } else {
                serde_json::json!({"workflow_runs":[run_json()]})
            })
        })
        .unwrap();
        assert_eq!(calls, 2);
        assert_eq!(w.runs.len(), 1);
    }
    #[test]
    fn missing_merge_identity_or_bad_response_cannot_be_no_runs() {
        assert!(poll_with(watch(),200,|_,_|Ok(serde_json::json!({"state":"closed","merged":true,"merged_at":"2026-10-17T10:00:00Z","merge_commit_sha":null,"base":{"ref":"main"}}))).is_err());
        let mut w = watch();
        w.merge_sha = Some("merge".into());
        assert!(poll_with(w, 200, |_, _| Ok(
            serde_json::json!({"message":"not authorized"})
        ))
        .is_err());
    }
}
