//! Account-level allowances, queried without starting an agent turn.
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use gpui::{Context, Entity};
use ide_core::AgentKind;
use serde::Deserialize;

use super::agent_chat::protocol::{agent_command_path_env, find_agent_cli_executable};

const REFRESH_INTERVAL: i64 = 300;

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct AllowanceWindow {
    pub label: String,
    pub used_percent: f64,
    pub resets_at: Option<i64>,
}

impl AllowanceWindow {
    pub fn remaining_percent(&self) -> f64 {
        100.0 - self.used_percent.clamp(0.0, 100.0)
    }

    pub fn expired(&self, now: i64) -> bool {
        self.resets_at.is_some_and(|reset| reset <= now)
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct AllowanceSnapshot {
    pub status: String,
    pub plan: Option<String>,
    #[serde(default)]
    pub windows: Vec<AllowanceWindow>,
    pub reset_credits: Option<u64>,
    pub account_fingerprint: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct ProviderAllowance {
    pub provider: AgentKind,
    pub snapshot: AllowanceSnapshot,
    pub refreshing: bool,
    pub updated_at: Option<i64>,
    pub refresh_failed: bool,
    last_attempt_at: Option<i64>,
}

impl ProviderAllowance {
    pub fn warning(&self, now: i64) -> bool {
        self.updated_at
            .is_some_and(|updated| now - updated <= REFRESH_INTERVAL)
            && !self.refresh_failed
            && self
                .snapshot
                .windows
                .iter()
                .any(|window| !window.expired(now) && window.remaining_percent() <= 20.0)
    }

    pub fn message(&self) -> &'static str {
        match self.snapshot.status.as_str() {
            "loading" => "Checking subscription usage…",
            "signed_out" => "Sign in to this agent to see subscription usage.",
            "no_subscription" => "Subscription limits are unavailable for this sign-in.",
            "update_required" => "Update the installed agent to read subscription usage.",
            "unsupported" if self.provider == AgentKind::Gemini => {
                "Gemini has not exposed account quotas through this connection yet."
            }
            "unsupported" => "This connection does not expose subscription allowances.",
            _ => "Usage unavailable. Refresh or check your account usage.",
        }
    }

    fn accept(&mut self, snapshot: AllowanceSnapshot, now: i64) {
        self.refreshing = false;
        // Retain stale data only when the provider confirmed the same account.
        // Changed or unverifiable authentication must clear the old allowance.
        let same_account = snapshot.account_fingerprint.is_some()
            && snapshot.account_fingerprint == self.snapshot.account_fingerprint;
        if snapshot.status == "unavailable" && same_account && !self.snapshot.windows.is_empty() {
            self.refresh_failed = true;
        } else {
            self.refresh_failed = false;
            self.updated_at = (snapshot.status == "ready").then_some(now);
            self.snapshot = snapshot;
        }
    }
}

pub(crate) struct SubscriptionUsageState {
    pub providers: Vec<ProviderAllowance>,
    pub open: bool,
}

impl SubscriptionUsageState {
    #[cfg(all(test, feature = "ui-layout-tests"))]
    pub fn in_memory(readings: Vec<(AgentKind, AllowanceSnapshot)>) -> Self {
        let now = chrono::Utc::now().timestamp();
        Self {
            providers: readings
                .into_iter()
                .map(|(provider, snapshot)| ProviderAllowance {
                    provider,
                    snapshot,
                    refreshing: false,
                    updated_at: Some(now),
                    refresh_failed: false,
                    last_attempt_at: Some(now),
                })
                .collect(),
            open: false,
        }
    }

    pub fn view(
        gemini_connected: bool,
        opencode_connected: bool,
        cx: &mut gpui::App,
    ) -> Entity<Self> {
        use gpui::AppContext as _;
        let state = cx.new(|_| Self {
            providers: [
                AgentKind::Claude,
                AgentKind::Codex,
                AgentKind::Gemini,
                AgentKind::OpenCode,
            ]
            .into_iter()
            .filter(|provider| match provider {
                AgentKind::Claude => find_agent_cli_executable("claude").is_some(),
                AgentKind::Codex => find_agent_cli_executable("codex").is_some(),
                AgentKind::Gemini => {
                    gemini_connected || super::agent_chat::protocol::gemini::provider_available()
                }
                AgentKind::OpenCode => opencode_connected,
            })
            .map(|provider| ProviderAllowance {
                provider,
                snapshot: AllowanceSnapshot {
                    status: if matches!(provider, AgentKind::Claude | AgentKind::Codex) {
                        "loading".into()
                    } else {
                        "unsupported".into()
                    },
                    ..Default::default()
                },
                refreshing: false,
                updated_at: None,
                refresh_failed: false,
                last_attempt_at: None,
            })
            .collect(),
            open: false,
        });
        state.update(cx, |this, cx| {
            this.refresh(true, cx);
            cx.spawn(async move |this, cx| loop {
                cx.background_executor()
                    .timer(Duration::from_secs(30))
                    .await;
                if this
                    .update(cx, |this, cx| {
                        this.refresh(false, cx);
                    })
                    .is_err()
                {
                    break;
                }
            })
            .detach();
        });
        state
    }

    pub fn set_open(&mut self, open: bool, cx: &mut Context<Self>) {
        self.open = open;
        if open {
            self.refresh(false, cx);
        }
        cx.notify();
    }

    /// Agents connected after launch join the same account popover.
    pub fn include_connected(&mut self, providers: &[AgentKind], cx: &mut Context<Self>) {
        let mut changed = false;
        for &provider in providers {
            if self.providers.iter().any(|row| row.provider == provider) {
                continue;
            }
            self.providers.push(ProviderAllowance {
                provider,
                snapshot: AllowanceSnapshot {
                    status: if matches!(provider, AgentKind::Claude | AgentKind::Codex) {
                        "loading".into()
                    } else {
                        "unsupported".into()
                    },
                    ..Default::default()
                },
                refreshing: false,
                updated_at: None,
                refresh_failed: false,
                last_attempt_at: None,
            });
            changed = true;
        }
        if changed {
            self.refresh(false, cx);
        }
    }

    pub fn refresh(&mut self, force: bool, cx: &mut Context<Self>) {
        let now = chrono::Utc::now().timestamp();
        for row in &mut self.providers {
            if row.refreshing || !matches!(row.provider, AgentKind::Claude | AgentKind::Codex) {
                continue;
            }
            let interval = if row
                .snapshot
                .windows
                .iter()
                .any(|window| window.expired(now))
            {
                60
            } else {
                REFRESH_INTERVAL
            };
            if !force
                && row
                    .last_attempt_at
                    .is_some_and(|checked| now - checked < interval)
            {
                continue;
            }
            row.refreshing = true;
            row.last_attempt_at = Some(now);
            let provider = row.provider;
            cx.spawn(async move |this, cx| {
                let snapshot = cx
                    .background_executor()
                    .spawn(async move { query_allowance(provider) })
                    .await;
                this.update(cx, |this, cx| {
                    if let Some(row) = this
                        .providers
                        .iter_mut()
                        .find(|row| row.provider == provider)
                    {
                        row.accept(snapshot, chrono::Utc::now().timestamp());
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        cx.notify();
    }
}

fn query_allowance(provider: AgentKind) -> AllowanceSnapshot {
    let unavailable = || AllowanceSnapshot {
        status: "unavailable".into(),
        ..Default::default()
    };
    let result = (|| -> Option<AllowanceSnapshot> {
        let name = if provider == AgentKind::Claude {
            "claude"
        } else {
            "codex"
        };
        let cli = find_agent_cli_executable(name)?;
        let node = find_agent_cli_executable("node")?;
        let script = super::agent_chat::protocol::claude_bridge_script_path()
            .ok()?
            .with_file_name("subscription_usage.mjs");
        let mut command = Command::new(node);
        command
            .arg(script)
            .arg(name)
            .arg(cli)
            .env("PATH", agent_command_path_env())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt as _;
            command.process_group(0);
        }
        let mut child = command.spawn().ok()?;
        let stdout = child.stdout.take()?;
        let (tx, rx) = crossbeam_channel::bounded(1);
        std::thread::spawn(move || {
            // The helper emits one small, sanitized JSON line.
            use std::io::Read as _;
            let mut line = String::new();
            let _ = BufReader::new(stdout.take(128 * 1024)).read_line(&mut line);
            let _ = tx.send(line);
        });
        let deadline = Instant::now() + Duration::from_secs(25);
        let reading = loop {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(line) => break serde_json::from_str(&line).ok(),
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break None,
                Err(_) if Instant::now() >= deadline => break None,
                Err(_) => {}
            }
        };
        // Reap the helper and its provider child even on timeout/window closure.
        #[cfg(unix)]
        unsafe {
            libc::kill(-(child.id() as i32), libc::SIGKILL);
        }
        let _ = child.kill();
        let _ = child.wait();
        reading
    })();
    result.unwrap_or_else(unavailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row() -> ProviderAllowance {
        ProviderAllowance {
            provider: AgentKind::Claude,
            snapshot: AllowanceSnapshot {
                status: "ready".into(),
                account_fingerprint: Some("account-a".into()),
                windows: vec![AllowanceWindow {
                    label: "Weekly".into(),
                    used_percent: 90.0,
                    resets_at: Some(2000),
                }],
                ..Default::default()
            },
            refreshing: false,
            updated_at: Some(1000),
            refresh_failed: false,
            last_attempt_at: Some(1000),
        }
    }

    #[test]
    fn failed_refresh_keeps_reading_but_suppresses_warning_and_account_change_clears_it() {
        let mut row = row();
        assert!(row.warning(1001));
        row.accept(
            AllowanceSnapshot {
                status: "unavailable".into(),
                account_fingerprint: Some("account-a".into()),
                ..Default::default()
            },
            1002,
        );
        assert_eq!(row.updated_at, Some(1000));
        assert_eq!(row.snapshot.windows.len(), 1);
        assert!(row.refresh_failed);
        assert!(!row.warning(1002));
        row.accept(
            AllowanceSnapshot {
                status: "signed_out".into(),
                ..Default::default()
            },
            1003,
        );
        assert!(row.snapshot.windows.is_empty());
    }

    #[test]
    fn expired_or_old_readings_do_not_warn_about_current_allowances() {
        let mut row = row();
        assert!(!row.warning(1400));
        row.updated_at = Some(2000);
        assert!(!row.warning(2000));
        assert!(row.snapshot.windows[0].expired(2000));
    }

    #[test]
    fn a_failed_query_for_a_changed_or_unverifiable_account_clears_the_previous_reading() {
        for account_fingerprint in [Some("account-b".into()), None] {
            let mut row = row();
            row.accept(
                AllowanceSnapshot {
                    status: "unavailable".into(),
                    account_fingerprint,
                    ..Default::default()
                },
                1002,
            );
            assert!(row.snapshot.windows.is_empty());
            assert_eq!(row.updated_at, None);
            assert!(!row.warning(1002));
        }
    }
}
