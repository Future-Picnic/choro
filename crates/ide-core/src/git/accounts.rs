use std::fs;
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::AppConfig;

const GITHUB_HOST: &str = "github.com";
const CREDENTIAL_ACCOUNT_ENV: &str = "CHORO_GIT_CREDENTIAL_ACCOUNT";
/// First argv entry that marks a Choro launch as a git credential-helper
/// invocation rather than an app start.
const CREDENTIAL_SENTINEL: &str = "choro-git-credential";
/// Only operations that can contact a remote get managed credentials; local
/// commands (`add`, `stash`, `merge`…) must never carry them.
const NETWORK_OPERATIONS: &[&str] = &["push", "pull", "fetch", "ls-remote", "clone"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubAccount {
    pub login: String,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRemote {
    pub name: String,
    pub url: String,
}

impl GitRemote {
    /// Strictly `https://` — a plaintext `http://github.com/` remote must
    /// never be managed, or the token would ride an unencrypted connection.
    pub fn is_github_https(&self) -> bool {
        self.url.starts_with("https://github.com/")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct GitAccountBinding {
    repository: String,
    remote: String,
    remote_url: String,
    account: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
struct GitAccountBindings {
    #[serde(default)]
    bindings: Vec<GitAccountBinding>,
}

impl GitAccountBindings {
    fn path() -> PathBuf {
        AppConfig::config_root().join("git-account-bindings.json")
    }

    fn load() -> Self {
        fs::read_to_string(Self::path())
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    fn save(&self) -> Result<()> {
        let path = Self::path();
        let parent = path
            .parent()
            .context("Git account bindings path has no parent")?;
        fs::create_dir_all(parent).context("Could not create Choro's config directory")?;
        let temporary = path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(self)
            .context("Could not serialize Git account bindings")?;
        fs::write(&temporary, json).context("Could not write Git account bindings")?;
        fs::rename(&temporary, &path).context("Could not save Git account bindings")?;
        Ok(())
    }
}

pub fn connected_github_accounts() -> Result<Vec<GitHubAccount>> {
    let mut command = github_cli_command()?;
    command.args(["auth", "status", "--hostname", GITHUB_HOST]);
    // `gh auth status` performs a network round-trip; behind a captive portal
    // an unbounded wait would latch the account menu in "Loading…" forever.
    let output = output_with_timeout(command, Duration::from_secs(10))
        .context("Could not inspect connected GitHub accounts")?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    let accounts = parse_auth_status(&text);
    if accounts.is_empty() {
        if output.status.success() {
            anyhow::bail!("No GitHub accounts are connected")
        }
        let message = text.trim();
        anyhow::bail!(if message.is_empty() {
            "No GitHub accounts are connected".to_string()
        } else {
            message.to_string()
        });
    }
    Ok(accounts)
}

fn parse_auth_status(text: &str) -> Vec<GitHubAccount> {
    let mut accounts = Vec::<GitHubAccount>::new();
    let mut pending = None;
    for line in text.lines().map(str::trim) {
        if line.contains("Logged in to ") {
            pending = None;
            if let Some((_, tail)) = line.split_once(" account ") {
                let login = tail
                    .split_whitespace()
                    .next()
                    .unwrap_or_default()
                    .trim_end_matches(|character: char| character == ')' || character == ',');
                if !login.is_empty() && !accounts.iter().any(|account| account.login == login) {
                    accounts.push(GitHubAccount {
                        login: login.to_string(),
                        active: false,
                    });
                    pending = Some(accounts.len() - 1);
                }
            }
        } else if line.contains(" account ") {
            // A header we don't track ("Failed to log in to … account bob") —
            // its trailing "Active account:" line must not hit a stale index.
            pending = None;
        } else if line.contains("Active account: true") {
            if let Some(index) = pending.take() {
                accounts[index].active = true;
            }
        }
    }
    accounts.sort_by_key(|account| !account.active);
    accounts
}

pub fn repository_remotes(repo_path: &Path) -> Vec<GitRemote> {
    git2::Repository::open(repo_path)
        .map(|repo| remotes_of(&repo))
        .unwrap_or_default()
}

/// In-process remote listing via libgit2 — reads `.git/config` directly, so it
/// is cheap enough to run inside the snapshot refresh (no subprocess spawns).
pub(crate) fn remotes_of(repo: &git2::Repository) -> Vec<GitRemote> {
    let Ok(names) = repo.remotes() else {
        return Vec::new();
    };
    names
        .iter()
        .filter_map(|name| {
            let name = name.ok().flatten()?;
            let remote = repo.find_remote(name).ok()?;
            let url = remote.url().ok()?.trim().to_string();
            (!url.is_empty()).then(|| GitRemote {
                name: name.to_string(),
                url,
            })
        })
        .collect()
}

pub fn primary_remote(repo_path: &Path) -> Option<GitRemote> {
    let repo = git2::Repository::open(repo_path).ok()?;
    primary_remote_of(&repo)
}

pub(crate) fn primary_remote_of(repo: &git2::Repository) -> Option<GitRemote> {
    let remotes = remotes_of(repo);
    remotes
        .iter()
        .find(|remote| remote.name == "origin")
        .cloned()
        .or_else(|| remotes.into_iter().next())
}

pub fn assigned_github_account(repo_path: &Path, remote: &GitRemote) -> Option<String> {
    assigned_account_for_identity(&repository_identity(repo_path), remote)
}

/// Snapshot-refresh variant: reuses an already-open repository so the lookup
/// stays fully in-process (one small JSON read, no git subprocess).
pub(crate) fn assigned_account_of(repo: &git2::Repository, remote: &GitRemote) -> Option<String> {
    assigned_account_for_identity(&identity_of(repo), remote)
}

fn assigned_account_for_identity(repository: &str, remote: &GitRemote) -> Option<String> {
    find_binding(&GitAccountBindings::load(), repository, remote)
}

fn find_binding(
    bindings: &GitAccountBindings,
    repository: &str,
    remote: &GitRemote,
) -> Option<String> {
    bindings
        .bindings
        .iter()
        .find(|binding| {
            binding.repository == repository
                && binding.remote == remote.name
                && normalized_remote_url(&binding.remote_url) == normalized_remote_url(&remote.url)
        })
        .map(|binding| binding.account.clone())
}

/// Bindings must survive cosmetic URL edits — `git remote set-url` dropping a
/// `.git` suffix or a trailing slash must not silently unbind the account.
fn normalized_remote_url(url: &str) -> String {
    let url = url.trim();
    let url = url.strip_suffix('/').unwrap_or(url);
    let url = url.strip_suffix(".git").unwrap_or(url);
    url.to_ascii_lowercase()
}

pub fn assign_github_account(
    repo_path: &Path,
    remote: &GitRemote,
    account: Option<&str>,
) -> Result<()> {
    let repository = repository_identity(repo_path);
    let mut stored = GitAccountBindings::load();
    stored.bindings.retain(|binding| {
        !(binding.repository == repository
            && binding.remote == remote.name
            && normalized_remote_url(&binding.remote_url) == normalized_remote_url(&remote.url))
    });
    if let Some(account) = account.filter(|account| !account.trim().is_empty()) {
        stored.bindings.push(GitAccountBinding {
            repository,
            remote: remote.name.clone(),
            remote_url: remote.url.clone(),
            account: account.to_string(),
        });
    }
    stored.save()
}

fn repository_identity(repo_path: &Path) -> String {
    git2::Repository::open(repo_path)
        .map(|repo| identity_of(&repo))
        .unwrap_or_else(|_| {
            repo_path
                .canonicalize()
                .unwrap_or_else(|_| repo_path.to_path_buf())
                .display()
                .to_string()
        })
}

/// The absolute common `.git` dir, matching `git rev-parse
/// --path-format=absolute --git-common-dir` (which earlier versions used to
/// write the bindings file) so existing account bindings keep resolving. The
/// trailing separator libgit2 appends is stripped for the same reason.
fn identity_of(repo: &git2::Repository) -> String {
    let common = repo.commondir().display().to_string();
    common.trim_end_matches('/').to_string()
}

pub(crate) fn configure_selected_account(command: &mut Command, repo_path: &Path, args: &[&str]) {
    let operation = args.first().copied().unwrap_or_default();
    if !NETWORK_OPERATIONS.contains(&operation) {
        return;
    }
    let Some(remote) = remote_for_operation(repo_path, args).filter(GitRemote::is_github_https)
    else {
        return;
    };
    let Some(account) = assigned_github_account(repo_path, &remote) else {
        return;
    };
    let Ok(executable) = std::env::current_exe() else {
        return;
    };

    // Everything is scoped to https://github.com: the empty helper resets
    // inherited helpers for that host only (other hosts keep the user's own
    // helpers), then Choro itself answers as a credential helper. Git gives
    // the helper `protocol=`/`host=` on stdin, so the release decision is
    // host-checked twice — by git's URL matching and again inside the helper —
    // and the token never appears in a URL or argv.
    let helper = format!(
        "!{} {CREDENTIAL_SENTINEL}",
        shell_quote(&executable.display().to_string())
    );
    command
        .arg("-c")
        .arg(format!("credential.https://{GITHUB_HOST}.helper="))
        .arg("-c")
        .arg(format!("credential.https://{GITHUB_HOST}.helper={helper}"))
        .arg("-c")
        .arg(format!(
            "credential.https://{GITHUB_HOST}.username={account}"
        ))
        .env(CREDENTIAL_ACCOUNT_ENV, account);
}

fn remote_for_operation(repo_path: &Path, args: &[&str]) -> Option<GitRemote> {
    let repo = git2::Repository::open(repo_path).ok()?;
    let operation = args.first().copied().unwrap_or_default();
    let remotes = remotes_of(&repo);
    if remotes.is_empty() {
        return None;
    }

    // An explicitly supplied remote always wins (`git push -u origin branch`).
    if matches!(operation, "push" | "pull" | "fetch") {
        if let Some(remote) = args.iter().skip(1).find_map(|argument| {
            (!argument.starts_with('-'))
                .then(|| remotes.iter().find(|remote| remote.name == *argument))
                .flatten()
        }) {
            return Some(remote.clone());
        }
    }

    // In-process equivalents of `git symbolic-ref` / `git config --get`. This
    // pre-flight runs before the operation's own timeout starts, so it must
    // never spawn a subprocess that could hang past it. HEAD is read without
    // resolving so an unborn branch still names itself.
    let branch = repo
        .find_reference("HEAD")
        .ok()
        .and_then(|head| {
            head.symbolic_target()
                .ok()
                .flatten()
                .map(str::to_string)
        })
        .and_then(|target| target.strip_prefix("refs/heads/").map(str::to_string));
    let config = repo.config().ok();
    let configured_remote = branch.as_deref().and_then(|branch| {
        let keys: Vec<String> = if operation == "push" {
            vec![
                format!("branch.{branch}.pushRemote"),
                "remote.pushDefault".to_string(),
                format!("branch.{branch}.remote"),
            ]
        } else {
            vec![format!("branch.{branch}.remote")]
        };
        let config = config.as_ref()?;
        keys.into_iter().find_map(|key| {
            config
                .get_string(&key)
                .ok()
                .map(|name| name.trim().to_string())
                .filter(|name| !name.is_empty() && name != ".")
        })
    });
    configured_remote
        .and_then(|name| remotes.iter().find(|remote| remote.name == name).cloned())
        .or_else(|| {
            remotes
                .iter()
                .find(|remote| remote.name == "origin")
                .cloned()
        })
        .or_else(|| remotes.into_iter().next())
}

/// Handles the short-lived subprocess invocation Git makes through the
/// `credential.helper` configured by [`configure_selected_account`]. Returns
/// `None` during an ordinary Choro launch — the sentinel argv entry, not an
/// environment variable, decides which mode this process is in.
pub fn handle_git_credential() -> Option<i32> {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some(CREDENTIAL_SENTINEL) {
        return None;
    }
    let operation = args.next().unwrap_or_default();

    // Git writes the request before reading our reply; drain it in every mode
    // so git never sees a broken pipe.
    let mut request = String::new();
    let _ = io::stdin().read_to_string(&mut request);
    if operation != "get" {
        return Some(0);
    }

    let Ok(account) = std::env::var(CREDENTIAL_ACCOUNT_ENV) else {
        eprintln!("Choro credential helper was invoked without an account binding");
        return Some(1);
    };
    if !credential_request_is_github(&request) {
        // Fail closed: the GitHub credential is released to github.com over
        // https only, no matter what URL rewriting sent git elsewhere.
        eprintln!("Choro credential helper refused a request for a non-GitHub host");
        return Some(1);
    }

    let output = github_cli_command().and_then(|mut command| {
        command.args([
            "auth",
            "token",
            "--hostname",
            GITHUB_HOST,
            "--user",
            &account,
        ]);
        output_with_timeout(command, Duration::from_secs(30))
            .context("Could not read the selected GitHub credential")
    });
    match output {
        Ok(output) if output.status.success() => {
            let token = String::from_utf8_lossy(&output.stdout);
            let _ = writeln!(io::stdout(), "username={account}");
            let _ = writeln!(io::stdout(), "password={}", token.trim());
            Some(0)
        }
        Ok(output) => {
            let message = String::from_utf8_lossy(&output.stderr);
            eprintln!("{}", message.trim());
            Some(1)
        }
        Err(error) => {
            eprintln!("{error:#}");
            Some(1)
        }
    }
}

/// True only for `protocol=https` to `host=github.com` (default port).
fn credential_request_is_github(request: &str) -> bool {
    let mut https = false;
    let mut github = false;
    for line in request.lines() {
        match line.split_once('=') {
            Some(("protocol", value)) => https = value.trim() == "https",
            Some(("host", value)) => {
                let host = value.trim();
                github = host == GITHUB_HOST || host == "github.com:443";
            }
            _ => {}
        }
    }
    https && github
}

/// Starts GitHub CLI's secure browser login in Terminal. The user explicitly
/// invokes this from the account menu, and GitHub CLI stores the result in the
/// operating-system credential store.
pub fn open_github_account_login() -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let gh = github_cli_path()?;
        let command = format!(
            "{} auth login --hostname {} --git-protocol https --web",
            shell_quote(&gh.display().to_string()),
            GITHUB_HOST
        );
        let script = format!(
            "tell application \"Terminal\" to do script {}",
            apple_quote(&command)
        );
        let status = Command::new("osascript")
            .args(["-e", &script])
            .status()
            .context("Could not open GitHub login in Terminal")?;
        if !status.success() {
            anyhow::bail!("Could not open GitHub login in Terminal");
        }
        return Ok(());
    }

    #[cfg(not(target_os = "macos"))]
    anyhow::bail!("Connect another account with `gh auth login`, then refresh the account list")
}

fn github_cli_command() -> Result<Command> {
    Ok(Command::new(github_cli_path()?))
}

fn github_cli_path() -> Result<PathBuf> {
    for candidate in [
        "/opt/homebrew/bin/gh",
        "/usr/local/bin/gh",
        "/usr/bin/gh",
        "/opt/local/bin/gh",
    ] {
        let path = PathBuf::from(candidate);
        if path.exists() {
            return Ok(path);
        }
    }
    let mut probe = Command::new("/bin/zsh");
    probe.args(["-lc", "command -v gh"]);
    let output =
        output_with_timeout(probe, Duration::from_secs(5)).context("Could not find GitHub CLI")?;
    let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    // `command -v` can echo an alias or bare function name; only an absolute
    // existing binary is acceptable as the credential source.
    if output.status.success() && path.is_absolute() && path.is_file() {
        Ok(path)
    } else {
        Err(anyhow!("GitHub CLI is required to connect GitHub accounts"))
    }
}

/// `Command::output` with a deadline: polls the child and kills it when the
/// timeout passes, so a wedged subprocess cannot latch panel state forever.
fn output_with_timeout(mut command: Command, timeout: Duration) -> Result<Output> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().context("Could not start the command")?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait().context("Could not poll the command")? {
            Some(_) => {
                return child
                    .wait_with_output()
                    .context("Could not read the command's output")
            }
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                anyhow::bail!("Timed out after {} seconds", timeout.as_secs());
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

fn git_output(repo_path: &Path, args: &[&str]) -> Result<Output> {
    crate::blocking_guard::debug_warn_if_ui_thread("git_output");
    Command::new("git")
        .args(args)
        .current_dir(repo_path)
        .output()
        .context("Could not inspect the Git repository")
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn apple_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(repo: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn parses_multiple_github_accounts_and_active_state() {
        let accounts = parse_auth_status(
            "github.com\n  ✓ Logged in to github.com account FuturePicnic (keyring)\n  - Active account: true\n\n  ✓ Logged in to github.com account liranRitmus (keyring)\n  - Active account: false\n",
        );

        assert_eq!(
            accounts,
            vec![
                GitHubAccount {
                    login: "FuturePicnic".into(),
                    active: true,
                },
                GitHubAccount {
                    login: "liranRitmus".into(),
                    active: false,
                },
            ]
        );
    }

    #[test]
    fn recognizes_only_github_https_remotes_for_managed_credentials() {
        assert!(GitRemote {
            name: "origin".into(),
            url: "https://github.com/acme/app.git".into(),
        }
        .is_github_https());
        assert!(!GitRemote {
            name: "origin".into(),
            url: "git@github.com:acme/app.git".into(),
        }
        .is_github_https());
        // Plaintext http must never be managed — the token would ride an
        // unencrypted connection.
        assert!(!GitRemote {
            name: "origin".into(),
            url: "http://github.com/acme/app.git".into(),
        }
        .is_github_https());
        assert!(!GitRemote {
            name: "origin".into(),
            url: "https://github.com.evil.example/acme/app.git".into(),
        }
        .is_github_https());
    }

    #[test]
    fn credential_helper_releases_only_to_github_over_https() {
        assert!(credential_request_is_github(
            "protocol=https\nhost=github.com\n"
        ));
        assert!(credential_request_is_github(
            "protocol=https\nhost=github.com:443\npath=acme/app.git\n"
        ));
        assert!(!credential_request_is_github(
            "protocol=http\nhost=github.com\n"
        ));
        assert!(!credential_request_is_github(
            "protocol=https\nhost=evil.example.com\n"
        ));
        assert!(!credential_request_is_github(
            "protocol=https\nhost=github.com.evil.example\n"
        ));
        assert!(!credential_request_is_github("host=github.com\n"));
        assert!(!credential_request_is_github(""));
    }

    #[test]
    fn local_operations_never_carry_managed_credentials() {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init"]);
        git(
            dir.path(),
            &["remote", "add", "origin", "https://github.com/me/app.git"],
        );
        let mut command = Command::new("git");
        configure_selected_account(&mut command, dir.path(), &["add", "-A"]);
        assert_eq!(command.get_args().count(), 0);
        assert!(!command
            .get_envs()
            .any(|(key, _)| key == CREDENTIAL_ACCOUNT_ENV));
    }

    #[test]
    fn unbound_repositories_get_no_credential_overrides() {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init"]);
        git(
            dir.path(),
            &["remote", "add", "origin", "https://github.com/me/app.git"],
        );
        let mut command = Command::new("git");
        configure_selected_account(&mut command, dir.path(), &["push", "origin", "main"]);
        assert_eq!(command.get_args().count(), 0);
    }

    #[test]
    fn bindings_survive_cosmetic_remote_url_edits() {
        let bindings = GitAccountBindings {
            bindings: vec![GitAccountBinding {
                repository: "/repo/.git".into(),
                remote: "origin".into(),
                remote_url: "https://github.com/acme/app.git".into(),
                account: "work".into(),
            }],
        };
        let edited = GitRemote {
            name: "origin".into(),
            url: "https://github.com/acme/app/".into(),
        };
        assert_eq!(
            find_binding(&bindings, "/repo/.git", &edited),
            Some("work".to_string())
        );
        assert_eq!(find_binding(&bindings, "/other/.git", &edited), None);
        let different_repo_url = GitRemote {
            name: "origin".into(),
            url: "https://github.com/acme/other.git".into(),
        };
        assert_eq!(
            find_binding(&bindings, "/repo/.git", &different_repo_url),
            None
        );
    }

    #[test]
    fn resolves_the_remote_git_will_use_for_each_network_action() {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init"]);
        git(dir.path(), &["symbolic-ref", "HEAD", "refs/heads/main"]);
        git(
            dir.path(),
            &["remote", "add", "origin", "https://github.com/me/app.git"],
        );
        git(
            dir.path(),
            &[
                "remote",
                "add",
                "upstream",
                "https://github.com/acme/app.git",
            ],
        );
        git(dir.path(), &["config", "branch.main.remote", "upstream"]);
        git(dir.path(), &["config", "branch.main.pushRemote", "origin"]);

        assert_eq!(
            remote_for_operation(dir.path(), &["fetch", "--prune"])
                .unwrap()
                .name,
            "upstream"
        );
        assert_eq!(
            remote_for_operation(dir.path(), &["push"]).unwrap().name,
            "origin"
        );
        assert_eq!(
            remote_for_operation(dir.path(), &["push", "-u", "upstream", "main"])
                .unwrap()
                .name,
            "upstream"
        );
    }
}
