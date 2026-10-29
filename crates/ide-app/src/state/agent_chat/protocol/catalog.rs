//! Bounded, inference-free probes using the same binary, home, project and
//! client identity as a conversation. Never read or copy OAuth credentials.
use super::*;
use ide_core::agents::codex::{parse_model_page, CodexModel};

pub(crate) const MIN_CODEX_VERSION: (u32, u32, u32) = (0, 159, 0);
const PROBE_TIMEOUT: Duration = Duration::from_secs(15);

pub(crate) fn version_number(text: &str) -> Option<(u32, u32, u32)> {
    let version = text
        .trim()
        .strip_prefix("codex-cli ")?
        .split_whitespace()
        .next()?;
    let mut parts = version.split('.');
    let result = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    parts.next().is_none().then_some(result)
}

pub(crate) fn executable_version(path: &Path) -> Option<(u32, u32, u32)> {
    let mut child = Command::new(path)
        .arg("--version")
        .env("PATH", command_path_env())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let output = child.wait_with_output().ok()?;
                return status
                    .success()
                    .then(|| version_number(&String::from_utf8_lossy(&output.stdout)))
                    .flatten();
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

pub(crate) struct CodexDiscovery {
    pub executable: PathBuf,
    pub version: (u32, u32, u32),
    pub models: Vec<CodexModel>,
}

pub(super) fn read_models(
    mut request: impl FnMut(Value) -> anyhow::Result<Value>,
) -> anyhow::Result<Vec<CodexModel>> {
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let mut models: Vec<CodexModel> = Vec::new();
    let mut cursor: Option<String> = None;
    let mut seen = HashSet::new();
    for _ in 0..100 {
        anyhow::ensure!(
            Instant::now() < deadline,
            "Codex model discovery timed out. Refresh models to retry."
        );
        let (entries, next) = parse_model_page(&request(
            json!({"limit":100,"includeHidden":false,"cursor":cursor}),
        )?)?;
        for entry in entries {
            if !models.iter().any(|model| model.id == entry.id) {
                models.push(entry);
            }
        }
        let Some(next) = next else {
            ide_core::agents::codex::resolve_model(&models, None)?;
            return Ok(models);
        };
        anyhow::ensure!(
            seen.insert(next.clone()),
            "Codex repeated a model catalog page. Update Codex and refresh models."
        );
        cursor = Some(next);
    }
    Err(anyhow!(
        "Codex returned too many model catalog pages. Update Codex and refresh models."
    ))
}

struct Probe {
    child: Child,
    input: ChildStdin,
    messages: Receiver<Value>,
    deadline: Instant,
    next_id: u64,
}

impl Drop for Probe {
    fn drop(&mut self) {
        terminate_child_process(&mut self.child);
    }
}

impl Probe {
    fn request(&mut self, method: &str, params: Value) -> anyhow::Result<Value> {
        self.next_id += 1;
        let id = self.next_id;
        writeln!(
            self.input,
            "{}",
            json!({"id":id,"method":method,"params":params})
        )?;
        self.input.flush()?;
        loop {
            let remaining = self.deadline.saturating_duration_since(Instant::now());
            anyhow::ensure!(
                !remaining.is_zero(),
                "Codex model discovery timed out. Refresh models to retry."
            );
            let message = self.messages.recv_timeout(remaining).context(
                "Codex did not finish checking its connection. Refresh models to retry.",
            )?;
            if message.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = message.get("error") {
                let message = error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("Codex rejected the connection check.");
                return Err(anyhow!(
                    "{}",
                    ide_core::agents::codex::readable_error(message)
                ));
            }
            return message
                .get("result")
                .cloned()
                .context("Codex returned an invalid connection response.");
        }
    }
}

pub(crate) fn discover(project_path: &Path) -> anyhow::Result<Option<CodexDiscovery>> {
    let Some(executable) = find_codex_app_server_executable() else {
        return Ok(None);
    };
    discover_using(executable, project_path, PROBE_TIMEOUT).map(Some)
}

fn discover_using(
    executable: PathBuf,
    project_path: &Path,
    timeout: Duration,
) -> anyhow::Result<CodexDiscovery> {
    let version = executable_version(&executable)
        .context("Could not read the Codex version. Update Codex and refresh models.")?;
    anyhow::ensure!(version >= MIN_CODEX_VERSION,
        "Codex {}.{}.{} at {} needs an update. Choro requires Codex 0.159.0 or newer. Update this installation, then refresh models.",
        version.0, version.1, version.2, executable.display());
    let mut command = Command::new(&executable);
    command
        .arg("app-server")
        .env("PATH", command_path_env())
        .current_dir(project_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command
        .spawn()
        .context("Could not start Codex to check its connection.")?;
    let input = child.stdin.take().context("Codex input unavailable")?;
    let output = child.stdout.take().context("Codex output unavailable")?;
    let (send, messages) = crossbeam_channel::bounded(32);
    thread::spawn(move || {
        for line in BufReader::new(output).lines() {
            let Ok(line) = line else {
                break;
            };
            if let Ok(value) = serde_json::from_str::<Value>(&line) {
                if send.send(value).is_err() {
                    break;
                }
            }
        }
    });
    let mut probe = Probe {
        child,
        input,
        messages,
        deadline: Instant::now() + timeout,
        next_id: 0,
    };
    probe.request("initialize", json!({"clientInfo":{"name":"choro","title":"Choro","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":true}}))?;
    writeln!(probe.input, "{}", json!({"method":"initialized"}))?;
    probe.input.flush()?;
    let account = probe.request("account/read", json!({"refreshToken":false}))?;
    anyhow::ensure!(
        !account.get("account").is_none_or(Value::is_null)
            || account.get("requiresOpenaiAuth").and_then(Value::as_bool) == Some(false),
        "Codex needs sign-in. Run `codex login`, then refresh models."
    );
    let models = read_models(|params| probe.request("model/list", params))?;
    Ok(CodexDiscovery {
        executable,
        version,
        models,
    })
}

pub(crate) fn discover_at(
    executable: PathBuf,
    project_path: &Path,
) -> anyhow::Result<CodexDiscovery> {
    discover_using(executable, project_path, PROBE_TIMEOUT)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn versions_are_compared_numerically_and_unknown_builds_are_not_assumed_compatible() {
        assert!(version_number("codex-cli 0.164.0\n").unwrap() >= MIN_CODEX_VERSION);
        assert!(version_number("codex-cli 0.153.0").unwrap() < MIN_CODEX_VERSION);
        assert!(version_number("codex-cli 0.99.0").unwrap() < MIN_CODEX_VERSION);
        assert_eq!(version_number("codex-cli unknown"), None);
        assert_eq!(version_number("codex-cli 0.164.0-beta"), None);
    }

    #[test]
    fn catalog_pagination_preserves_ids_and_rejects_repeated_cursors() {
        let mut requests = Vec::new();
        let models = read_models(|params| {
            requests.push(params.clone());
            Ok(if requests.len() == 1 {
                json!({"data":[{"model":"first"}],"nextCursor":"page2"})
            } else {
                json!({"data":[{"model":"second","isDefault":true}],"nextCursor":null})
            })
        })
        .unwrap();
        assert_eq!(
            models
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            ["first", "second"]
        );
        assert!(requests[0]["cursor"].is_null());
        assert_eq!(requests[1]["cursor"], "page2");
        let error = read_models(|_| Ok(json!({"data":[{"model":"same"}],"nextCursor":"repeated"})))
            .unwrap_err();
        assert!(error.to_string().contains("repeated"));
    }

    #[cfg(unix)]
    fn mock_codex(dir: &Path, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("codex");
        fs::write(&path, format!("#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'codex-cli 0.164.0'; exit 0; fi\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        path
    }

    #[test]
    #[cfg(unix)]
    fn probe_uses_the_selected_binary_and_never_starts_an_inference_turn() {
        let dir = tempfile::tempdir().unwrap();
        let executable = mock_codex(
            dir.path(),
            r#"
while IFS= read -r request; do
  case "$request" in
    *'"method":"initialize"'*) echo '{"id":1,"result":{}}' ;;
    *'"method":"initialized"'*) ;;
    *'"method":"account/read"'*) echo '{"id":2,"result":{"account":{"type":"chatgpt"}}}' ;;
    *'"method":"model/list"'*) echo '{"id":3,"result":{"data":[{"model":"future-model","displayName":"Future","isDefault":true}]}}' ;;
    *) exit 1 ;;
  esac
done
"#,
        );
        let discovery =
            discover_using(executable.clone(), dir.path(), Duration::from_secs(2)).unwrap();
        assert_eq!(discovery.executable, executable);
        assert_eq!(discovery.version, (0, 164, 0));
        assert_eq!(discovery.models[0].id, "future-model");
    }

    #[test]
    #[cfg(unix)]
    fn unresponsive_probe_has_a_deadline_and_is_terminated() {
        let dir = tempfile::tempdir().unwrap();
        let executable = mock_codex(dir.path(), "while IFS= read -r request; do :; done");
        let start = Instant::now();
        let error = discover_using(executable, dir.path(), Duration::from_millis(50))
            .err()
            .unwrap();
        assert!(error.to_string().contains("Codex"));
        assert!(start.elapsed() < Duration::from_secs(3));
    }

    #[test]
    #[ignore = "requires an installed, authenticated Codex; reads catalog without inference"]
    fn installed_codex_catalog_smoke() {
        let discovery = discover(&std::env::current_dir().unwrap())
            .unwrap()
            .expect("Codex must be installed");
        assert!(discovery.version >= MIN_CODEX_VERSION);
        assert!(!discovery.models.is_empty());
    }
}
