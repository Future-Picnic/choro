use super::*;

pub(super) fn git_output(repo: &Path, args: &[&str]) -> anyhow::Result<String> {
    let _git_permit = ide_core::git::BackgroundGitPermit::acquire();
    let output = Command::new("git").args(args).current_dir(repo).output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        anyhow::bail!(
            "git {} failed{}",
            args.join(" "),
            if stderr.is_empty() {
                String::new()
            } else {
                format!(": {stderr}")
            }
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn validate_codex_cli_path(path: PathBuf, source: &str) -> anyhow::Result<PathBuf> {
    if !path.is_absolute() {
        anyhow::bail!("{source} must point to an absolute Codex CLI path");
    }
    let meta = std::fs::metadata(&path)
        .map_err(|error| anyhow::anyhow!("{source} is not accessible: {error}"))?;
    if !meta.is_file() {
        anyhow::bail!("{source} is not a file: {}", path.display());
    }
    Ok(path)
}

fn codex_cli_path() -> anyhow::Result<PathBuf> {
    if let Some(path) = std::env::var_os("CODEX_CLI").filter(|path| !path.is_empty()) {
        return validate_codex_cli_path(PathBuf::from(path), "CODEX_CLI");
    }

    let mut candidates = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        candidates.push(home.join(".local/bin/codex"));
        candidates.push(home.join(".codex/bin/codex"));
    }
    candidates.extend([
        PathBuf::from("/opt/homebrew/bin/codex"),
        PathBuf::from("/usr/local/bin/codex"),
        PathBuf::from("/usr/bin/codex"),
    ]);

    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Codex CLI not found in trusted locations. Set CODEX_CLI to an absolute path."
            )
        })
}

fn codex_output_path() -> PathBuf {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    std::env::temp_dir().join(format!(
        "choro-codex-generation-{}-{millis}.txt",
        std::process::id()
    ))
}

const CODEX_QUICK_ASK_PERMISSION_CONFIG: [&str; 3] = [
    "default_permissions=\"quick-ask\"",
    "permissions.quick-ask.extends=\":read-only\"",
    "permissions.quick-ask.network.enabled=true",
];

fn codex_exec_command(
    working_directory: &Path,
    output_path: &Path,
    model: &str,
    images: &[PathBuf],
    access: GenerationAccess,
) -> anyhow::Result<Command> {
    let mut command = Command::new(codex_cli_path()?);
    command
        .arg("exec")
        .arg("--ignore-user-config")
        .arg("--ignore-rules")
        .arg("--skip-git-repo-check");
    match access {
        GenerationAccess::ToolFree => {
            command.arg("--sandbox").arg("read-only");
        }
        GenerationAccess::QuickAsk => {
            for config in CODEX_QUICK_ASK_PERMISSION_CONFIG {
                command.arg("--config").arg(config);
            }
        }
    }
    command
        .arg("--model")
        .arg(model)
        .arg("--config")
        .arg(format!(
            "model_reasoning_effort=\"{CODEX_GENERATION_REASONING_EFFORT}\""
        ))
        .arg("--config")
        .arg("approval_policy=\"never\"")
        .arg("--ephemeral")
        .arg("--color")
        .arg("never")
        .arg("--output-last-message")
        .arg(output_path);
    for image in images {
        command.arg("--image").arg(image);
    }
    command
        .arg("-")
        .current_dir(working_directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    Ok(command)
}

fn run_codex_generation(
    repo: &Path,
    model: &str,
    prompt: String,
    images: &[PathBuf],
    timeout: Duration,
    access: GenerationAccess,
) -> anyhow::Result<String> {
    let output_path = codex_output_path();
    let mut child = codex_exec_command(repo, &output_path, model, images, access)?
        .spawn()
        .map_err(|error| anyhow::anyhow!("Failed to start Codex CLI: {error}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(prompt.as_bytes())?;
    }

    let stdout_reader = child.stdout.take().map(|mut pipe| {
        std::thread::spawn(move || {
            let mut output = String::new();
            pipe.read_to_string(&mut output).ok();
            output
        })
    });
    let stderr_reader = child.stderr.take().map(|mut pipe| {
        std::thread::spawn(move || {
            let mut output = String::new();
            pipe.read_to_string(&mut output).ok();
            output
        })
    });
    let wait_result = wait_with_timeout(&mut child, timeout, "Codex");
    let stdout = stdout_reader
        .and_then(|reader| reader.join().ok())
        .unwrap_or_default();
    let stderr = stderr_reader
        .and_then(|reader| reader.join().ok())
        .unwrap_or_default();

    let output = std::fs::read_to_string(&output_path).unwrap_or_else(|_| stdout.clone());
    let _ = std::fs::remove_file(&output_path);
    let status = wait_result?;

    if !status.success() {
        let stderr = stderr.trim();
        anyhow::bail!(
            "Codex failed{}",
            if stderr.is_empty() {
                format!(" with status {status}")
            } else {
                format!(": {stderr}")
            }
        );
    }

    Ok(output)
}

fn generation_cli_path(provider: AgentKind) -> anyhow::Result<PathBuf> {
    if provider == AgentKind::Codex {
        return codex_cli_path();
    }
    let (env_name, executable) = match provider {
        AgentKind::Claude => ("CLAUDE_CLI", "claude"),
        AgentKind::OpenCode => ("OPENCODE_CLI", "opencode"),
        AgentKind::Codex => unreachable!(),
    };
    if let Some(path) = std::env::var_os(env_name).filter(|path| !path.is_empty()) {
        return validate_generation_cli_path(PathBuf::from(path), env_name, provider);
    }

    let path = crate::state::agent_chat::protocol::find_agent_cli_executable(executable)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "{} CLI not found. Install it or set {env_name} to an absolute path.",
                provider.label()
            )
        })?;
    std::fs::canonicalize(&path).map_err(|error| {
        anyhow::anyhow!(
            "{} CLI path is not accessible at {}: {error}",
            provider.label(),
            path.display()
        )
    })
}

fn validate_generation_cli_path(
    path: PathBuf,
    source: &str,
    provider: AgentKind,
) -> anyhow::Result<PathBuf> {
    if !path.is_absolute() {
        anyhow::bail!(
            "{source} must point to an absolute {} CLI path",
            provider.label()
        );
    }
    let metadata = std::fs::metadata(&path)
        .map_err(|error| anyhow::anyhow!("{source} is not accessible: {error}"))?;
    if !metadata.is_file() {
        anyhow::bail!("{source} is not a file: {}", path.display());
    }
    Ok(path)
}

const MAX_GENERATION_IMAGES: usize = 5;
const MAX_GENERATION_IMAGE_BYTES: u64 = 10 * 1024 * 1024;
const MAX_GENERATION_IMAGE_BYTES_TOTAL: u64 = 25 * 1024 * 1024;

fn generation_image_media_type(path: &Path) -> Option<&'static str> {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => Some("image/png"),
        Some("jpg" | "jpeg") => Some("image/jpeg"),
        Some("gif") => Some("image/gif"),
        Some("webp") => Some("image/webp"),
        _ => None,
    }
}

fn validate_generation_images(images: &[PathBuf]) -> anyhow::Result<Vec<PathBuf>> {
    anyhow::ensure!(
        images.len() <= MAX_GENERATION_IMAGES,
        "Attach at most {MAX_GENERATION_IMAGES} images"
    );
    let mut total_bytes = 0_u64;
    let mut validated = Vec::with_capacity(images.len());
    for path in images {
        anyhow::ensure!(
            generation_image_media_type(path).is_some(),
            "Quick Ask supports PNG, JPEG, GIF, and WebP images"
        );
        let path = std::fs::canonicalize(path)
            .map_err(|error| anyhow::anyhow!("Could not open {}: {error}", path.display()))?;
        let metadata = std::fs::metadata(&path)?;
        anyhow::ensure!(metadata.is_file(), "{} is not a file", path.display());
        anyhow::ensure!(
            metadata.len() <= MAX_GENERATION_IMAGE_BYTES,
            "{} is larger than 10 MB",
            path.display()
        );
        total_bytes = total_bytes.saturating_add(metadata.len());
        anyhow::ensure!(
            total_bytes <= MAX_GENERATION_IMAGE_BYTES_TOTAL,
            "Attached images exceed the 25 MB total limit"
        );
        validated.push(path);
    }
    Ok(validated)
}

fn claude_stream_json_input(prompt: &str, images: &[PathBuf]) -> anyhow::Result<String> {
    use base64::Engine as _;

    let mut content = vec![serde_json::json!({ "type": "text", "text": prompt })];
    for path in images {
        let media_type = generation_image_media_type(path)
            .ok_or_else(|| anyhow::anyhow!("Unsupported image: {}", path.display()))?;
        let bytes = std::fs::read(path)?;
        let data = base64::engine::general_purpose::STANDARD.encode(bytes);
        content.push(serde_json::json!({
            "type": "image",
            "source": {
                "type": "base64",
                "media_type": media_type,
                "data": data,
            }
        }));
    }
    let message = serde_json::json!({
        "type": "user",
        "message": { "role": "user", "content": content }
    });
    Ok(format!("{}\n", serde_json::to_string(&message)?))
}

fn run_streamed_generation(
    generation_agent: &GenerationAgent,
    repo: &Path,
    prompt: String,
    timeout: Duration,
) -> anyhow::Result<String> {
    run_streamed_generation_with_images(generation_agent, repo, prompt, &[], timeout)
}

fn run_streamed_generation_with_images(
    generation_agent: &GenerationAgent,
    _repo: &Path,
    prompt: String,
    images: &[PathBuf],
    timeout: Duration,
) -> anyhow::Result<String> {
    // Generation prompts already contain every diff and metadata field the
    // model needs. Running from an empty disposable directory ensures that a
    // provider cannot mutate the user's repository, even if its local defaults
    // or future behavior accidentally expose an action-capable tool.
    let sandbox = tempfile::Builder::new()
        .prefix("choro-text-generation-")
        .tempdir()
        .map_err(|error| {
            anyhow::anyhow!("failed to create an isolated generation directory: {error}")
        })?;
    let working_directory = sandbox.path();
    let model = generation_agent.model_cli_value().ok_or_else(|| {
        anyhow::anyhow!(
            "No model is configured for {} generation",
            generation_agent.provider.label()
        )
    })?;
    let images = validate_generation_images(images)?;
    if generation_agent.provider == AgentKind::Codex {
        return run_codex_generation(
            working_directory,
            model,
            prompt,
            &images,
            timeout,
            GenerationAccess::ToolFree,
        );
    }

    match generation_agent.provider {
        AgentKind::Claude => run_claude_generation(
            working_directory,
            model,
            prompt,
            &images,
            timeout,
            GenerationAccess::ToolFree,
        ),
        AgentKind::OpenCode => run_open_code_generation(
            working_directory,
            model,
            prompt,
            &images,
            timeout,
            GenerationAccess::ToolFree,
        ),
        AgentKind::Codex => unreachable!(),
    }
}

/// Read-only, tool-free one-shot generation shared by app-owned features.
/// The provider runs in an empty disposable directory and cannot approve or
/// mutate a user's repository.
pub(crate) fn run_safe_text_generation(
    generation_agent: &GenerationAgent,
    prompt: String,
    timeout: Duration,
) -> anyhow::Result<String> {
    run_streamed_generation(generation_agent, Path::new("."), prompt, timeout)
}

/// Run Quick Ask with inspection and network tools inside the selected project,
/// while provider permissions prevent file edits and Git mutations. General
/// questions use the same tool policy from an empty disposable directory.
pub(crate) fn run_quick_ask_generation_with_images(
    generation_agent: &GenerationAgent,
    project_root: Option<&Path>,
    prompt: String,
    images: &[PathBuf],
    timeout: Duration,
) -> anyhow::Result<String> {
    let sandbox = project_root
        .is_none()
        .then(|| {
            tempfile::Builder::new()
                .prefix("choro-quick-ask-")
                .tempdir()
        })
        .transpose()
        .map_err(|error| {
            anyhow::anyhow!("failed to create an isolated Quick Ask directory: {error}")
        })?;
    let working_directory = project_root.unwrap_or_else(|| {
        sandbox
            .as_ref()
            .expect("general Quick Ask sandbox must exist")
            .path()
    });
    let model = generation_agent.model_cli_value().ok_or_else(|| {
        anyhow::anyhow!(
            "No model is configured for Quick Ask with {}",
            generation_agent.provider.label()
        )
    })?;
    let images = validate_generation_images(images)?;

    match generation_agent.provider {
        AgentKind::Codex => run_codex_generation(
            working_directory,
            model,
            prompt,
            &images,
            timeout,
            GenerationAccess::QuickAsk,
        ),
        AgentKind::Claude => run_claude_generation(
            working_directory,
            model,
            prompt,
            &images,
            timeout,
            GenerationAccess::QuickAsk,
        ),
        AgentKind::OpenCode => run_open_code_generation(
            working_directory,
            model,
            prompt,
            &images,
            timeout,
            GenerationAccess::QuickAsk,
        ),
    }
}

/// Run the configured small-writing model in the same isolated, no-tools path
/// used by commit, pull-request, Riff, and memory generation.
pub(crate) fn generate_one_shot_text(
    generation_agent: &GenerationAgent,
    working_directory: &Path,
    prompt: String,
    timeout: Duration,
) -> anyhow::Result<String> {
    run_streamed_generation(generation_agent, working_directory, prompt, timeout)
}

fn run_claude_generation(
    working_directory: &Path,
    model: &str,
    prompt: String,
    images: &[PathBuf],
    timeout: Duration,
    access: GenerationAccess,
) -> anyhow::Result<String> {
    let input_body = if images.is_empty() {
        prompt
    } else {
        claude_stream_json_input(&prompt, images)?
    };
    let executable = generation_cli_path(AgentKind::Claude)?;
    let mut command = provider_generation_command(&executable, working_directory, access);
    command.args([
        "--print",
        "--safe-mode",
        "--no-session-persistence",
        "--disable-slash-commands",
        "--strict-mcp-config",
        "--permission-mode",
        "dontAsk",
    ]);
    match access {
        GenerationAccess::ToolFree => {
            command.args(["--tools", ""]);
        }
        GenerationAccess::QuickAsk => {
            command.args([
                "--restricted",
                "--tools",
                "Bash,Read,Glob,Grep,WebFetch,WebSearch",
                "--disallowed-tools",
                "Write,Edit,NotebookEdit",
            ]);
        }
    }
    command.args(["--output-format", "text", "--model", model]);
    if !images.is_empty() {
        command.args(["--input-format", "stream-json"]);
    }
    command
        .env(
            "PATH",
            crate::state::agent_chat::protocol::agent_command_path_env(),
        )
        .current_dir(working_directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| anyhow::anyhow!("Failed to start Claude CLI: {error}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input_body.as_bytes())?;
    }
    finish_captured_generation(child, timeout, "Claude")
}

const OPENCODE_TEXT_GENERATION_CONFIG: &str = r#"{
    "permission": { "*": "deny" },
    "agent": {
        "choro-text-generation": {
            "description": "Return text for Choro without taking actions",
            "mode": "primary",
            "permission": { "*": "deny" }
        }
    },
    "share": "disabled",
    "autoupdate": false,
    "snapshot": false,
    "lsp": false,
    "formatter": false
}"#;

const OPENCODE_QUICK_ASK_CONFIG: &str = r#"{
    "permission": { "*": "deny" },
    "agent": {
        "choro-quick-ask": {
            "description": "Investigate and answer without changing files or Git state",
            "mode": "primary",
            "permission": {
                "*": "deny",
                "read": "allow",
                "glob": "allow",
                "grep": "allow",
                "webfetch": "allow",
                "websearch": "allow",
                "bash": {
                    "*": "deny",
                    "pwd": "allow",
                    "ls": "allow",
                    "ls *": "allow",
                    "rg *": "allow",
                    "grep *": "allow",
                    "find *": "allow",
                    "fd *": "allow",
                    "head *": "allow",
                    "tail *": "allow",
                    "wc *": "allow",
                    "file *": "allow",
                    "stat *": "allow",
                    "du *": "allow",
                    "tree *": "allow",
                    "which *": "allow",
                    "command -v *": "allow",
                    "git status*": "allow",
                    "git diff*": "allow",
                    "git log*": "allow",
                    "git show*": "allow",
                    "git rev-parse*": "allow",
                    "git ls-files*": "allow",
                    "git branch --show-current": "allow",
                    "git branch --list*": "allow",
                    "git remote -v": "allow",
                    "ps *": "allow",
                    "lsof *": "allow",
                    "uname *": "allow",
                    "sw_vers*": "allow",
                    "date": "allow"
                }
            }
        }
    },
    "share": "disabled",
    "autoupdate": false,
    "snapshot": false,
    "lsp": false,
    "formatter": false
}"#;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GenerationAccess {
    ToolFree,
    QuickAsk,
}

fn provider_generation_command(
    executable: &Path,
    working_directory: &Path,
    access: GenerationAccess,
) -> Command {
    #[cfg(target_os = "macos")]
    if access == GenerationAccess::QuickAsk {
        let protected_path = working_directory
            .to_string_lossy()
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
        let profile = format!(
            "(version 1)\n(allow default)\n(deny file-write* (subpath \"{protected_path}\"))"
        );
        let mut command = Command::new("/usr/bin/sandbox-exec");
        command.arg("-p").arg(profile).arg(executable);
        return command;
    }

    Command::new(executable)
}

fn configure_open_code_generation(
    command: &mut Command,
    working_directory: &Path,
    access: GenerationAccess,
) {
    let config = match access {
        GenerationAccess::ToolFree => OPENCODE_TEXT_GENERATION_CONFIG,
        GenerationAccess::QuickAsk => OPENCODE_QUICK_ASK_CONFIG,
    };
    command
        .env(
            "PATH",
            crate::state::agent_chat::protocol::agent_command_path_env(),
        )
        .env("OPENCODE_CONFIG_CONTENT", config)
        .env("OPENCODE_DISABLE_CLAUDE_CODE", "1")
        .current_dir(working_directory);
}

fn run_open_code_generation(
    working_directory: &Path,
    model: &str,
    prompt: String,
    images: &[PathBuf],
    timeout: Duration,
    access: GenerationAccess,
) -> anyhow::Result<String> {
    let executable = generation_cli_path(AgentKind::OpenCode)?;
    let temporary_title = format!("choro-generation-{}", uuid::Uuid::new_v4().simple());
    let agent_name = match access {
        GenerationAccess::ToolFree => "choro-text-generation",
        GenerationAccess::QuickAsk => "choro-quick-ask",
    };
    let mut command = provider_generation_command(&executable, working_directory, access);
    command.args([
        "--pure",
        "run",
        "--format",
        "json",
        "--model",
        model,
        "--agent",
        agent_name,
        "--title",
        &temporary_title,
    ]);
    for image in images {
        command.arg("--file").arg(image);
    }
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_open_code_generation(&mut command, working_directory, access);
    let mut child = command
        .spawn()
        .map_err(|error| anyhow::anyhow!("Failed to start OpenCode CLI: {error}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(prompt.as_bytes())?;
    }

    let stdout_reader = child.stdout.take().map(|mut pipe| {
        std::thread::spawn(move || {
            let mut output = String::new();
            pipe.read_to_string(&mut output).ok();
            output
        })
    });
    let stderr_reader = child.stderr.take().map(|mut pipe| {
        std::thread::spawn(move || {
            let mut output = String::new();
            pipe.read_to_string(&mut output).ok();
            output
        })
    });
    let wait_result = wait_with_timeout(&mut child, timeout, "OpenCode");
    let stdout = stdout_reader
        .and_then(|reader| reader.join().ok())
        .unwrap_or_default();
    let stderr = stderr_reader
        .and_then(|reader| reader.join().ok())
        .unwrap_or_default();
    let (reported_session_id, generated_text) = parse_open_code_generation_output(&stdout);
    let cleanup_session_id = reported_session_id.or_else(|| {
        find_open_code_session_by_title(&executable, working_directory, &temporary_title)
            .map_err(|error| {
                eprintln!("failed to locate temporary OpenCode generation session: {error:#}")
            })
            .ok()
            .flatten()
    });
    let cleanup_result = cleanup_session_id.as_deref().map(|session_id| {
        delete_open_code_generation_session(&executable, working_directory, session_id)
    });

    let status = wait_result?;
    if !status.success() {
        let stderr = stderr.trim();
        anyhow::bail!(
            "OpenCode failed{}",
            if stderr.is_empty() {
                format!(" with status {status}")
            } else {
                format!(": {stderr}")
            }
        );
    }
    if let Some(Err(error)) = cleanup_result {
        return Err(error);
    }
    if cleanup_session_id.is_none() {
        anyhow::bail!(
            "OpenCode did not report its temporary session id, so Choro could not guarantee cleanup"
        );
    }
    if generated_text.is_empty() {
        anyhow::bail!("OpenCode returned no completed text response");
    }
    Ok(generated_text)
}

fn finish_captured_generation(
    mut child: std::process::Child,
    timeout: Duration,
    provider: &str,
) -> anyhow::Result<String> {
    let stdout_reader = child.stdout.take().map(|mut pipe| {
        std::thread::spawn(move || {
            let mut output = String::new();
            pipe.read_to_string(&mut output).ok();
            output
        })
    });
    let stderr_reader = child.stderr.take().map(|mut pipe| {
        std::thread::spawn(move || {
            let mut output = String::new();
            pipe.read_to_string(&mut output).ok();
            output
        })
    });
    let wait_result = wait_with_timeout(&mut child, timeout, provider);
    let stdout = stdout_reader
        .and_then(|reader| reader.join().ok())
        .unwrap_or_default();
    let stderr = stderr_reader
        .and_then(|reader| reader.join().ok())
        .unwrap_or_default();
    let status = wait_result?;
    if !status.success() {
        let stderr = stderr.trim();
        let stdout = stdout.trim();
        let detail = if !stderr.is_empty() { stderr } else { stdout };
        anyhow::bail!(
            "{provider} failed{}",
            if detail.is_empty() {
                format!(" with status {status}")
            } else {
                format!(": {detail}")
            }
        );
    }
    Ok(stdout)
}

fn parse_open_code_generation_output(stdout: &str) -> (Option<String>, String) {
    let mut session_id = None;
    let mut text_parts = Vec::new();
    for line in stdout.lines().filter(|line| !line.trim().is_empty()) {
        let Ok(event) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if session_id.is_none() {
            session_id = event
                .get("sessionID")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string);
        }
        if event.get("type").and_then(serde_json::Value::as_str) == Some("text") {
            if let Some(text) = event
                .get("part")
                .and_then(|part| part.get("text"))
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
            {
                text_parts.push(text.to_string());
            }
        }
    }
    (session_id, text_parts.join("\n"))
}

fn find_open_code_session_by_title(
    executable: &Path,
    working_directory: &Path,
    title: &str,
) -> anyhow::Result<Option<String>> {
    let mut command = Command::new(executable);
    command
        .args(["--pure", "session", "list", "--format", "json"])
        .stdin(Stdio::null());
    configure_open_code_generation(&mut command, working_directory, GenerationAccess::ToolFree);
    let output = command
        .output()
        .map_err(|error| anyhow::anyhow!("Failed to list OpenCode sessions: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        anyhow::bail!(
            "OpenCode session listing failed{}",
            if stderr.is_empty() {
                String::new()
            } else {
                format!(": {stderr}")
            }
        );
    }
    let sessions: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| anyhow::anyhow!("OpenCode returned invalid session-list JSON: {error}"))?;
    Ok(sessions.as_array().and_then(|sessions| {
        sessions.iter().find_map(|session| {
            (session.get("title").and_then(serde_json::Value::as_str) == Some(title))
                .then(|| session.get("id").and_then(serde_json::Value::as_str))
                .flatten()
                .map(str::to_string)
        })
    }))
}

fn delete_open_code_generation_session(
    executable: &Path,
    working_directory: &Path,
    session_id: &str,
) -> anyhow::Result<()> {
    let mut command = Command::new(executable);
    command
        .args(["--pure", "session", "delete", session_id])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    configure_open_code_generation(&mut command, working_directory, GenerationAccess::ToolFree);
    let output = command
        .output()
        .map_err(|error| anyhow::anyhow!("Failed to clean up OpenCode session: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        anyhow::bail!(
            "OpenCode generated the text but its temporary session could not be deleted{}",
            if stderr.is_empty() {
                String::new()
            } else {
                format!(": {stderr}")
            }
        );
    }
    Ok(())
}

fn truncate_chars(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut out: String = text.chars().take(limit).collect();
    out.push_str("\n\n[diff truncated]");
    out
}

const PULL_REQUEST_DIFF_CHAR_LIMIT: usize = 120_000;

/// Keep a representative slice of every changed file when a large diff must
/// be shortened. A simple prefix disproportionately describes whichever files
/// Git happens to print first and can hide entire subsystems from PR copy.
fn truncate_diff_across_files(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }

    let mut starts = vec![0];
    starts.extend(text.match_indices("\ndiff --").map(|(index, _)| index + 1));
    starts.sort_unstable();
    starts.dedup();

    if starts.len() <= 1 {
        return truncate_chars(text, limit);
    }

    let sections = starts
        .iter()
        .copied()
        .enumerate()
        .map(|(position, start)| {
            let end = starts.get(position + 1).copied().unwrap_or(text.len());
            &text[start..end]
        })
        .collect::<Vec<_>>();
    let lengths = sections
        .iter()
        .map(|section| section.chars().count())
        .collect::<Vec<_>>();
    let mut allocations = vec![0; sections.len()];
    let mut pending = (0..sections.len()).collect::<Vec<_>>();
    let mut remaining = limit;

    while !pending.is_empty() && remaining > 0 {
        let share = remaining / pending.len();
        if share == 0 {
            break;
        }
        let completed = pending
            .iter()
            .copied()
            .filter(|index| lengths[*index] <= share)
            .collect::<Vec<_>>();
        if completed.is_empty() {
            for (position, index) in pending.iter().copied().enumerate() {
                allocations[index] = share + usize::from(position < remaining % pending.len());
            }
            break;
        }
        for index in &completed {
            allocations[*index] = lengths[*index];
            remaining = remaining.saturating_sub(lengths[*index]);
        }
        pending.retain(|index| !completed.contains(index));
    }

    let mut output = String::with_capacity(limit + starts.len() * 32);
    for (section, allocation) in sections.into_iter().zip(allocations) {
        output.push_str(&truncate_chars(section, allocation));
        if !output.ends_with('\n') {
            output.push('\n');
        }
    }
    output.push_str("\n[large diff sampled across every changed file]");
    output
}

fn build_commit_prompt(repo: &Path, use_staged: bool) -> anyhow::Result<String> {
    let scope = if use_staged {
        "staged changes only"
    } else {
        "tracked working-tree changes"
    };
    let (stat_args, diff_args): (&[&str], &[&str]) = if use_staged {
        (&["diff", "--cached", "--stat"], &["diff", "--cached"])
    } else {
        (&["diff", "HEAD", "--stat"], &["diff", "HEAD", "--"])
    };
    let stat = git_output(repo, stat_args)?;
    let diff = git_output(repo, diff_args)?;
    if diff.trim().is_empty() {
        anyhow::bail!("No diff available for commit message generation");
    }
    let history = git_output(repo, &["log", "-8", "--pretty=format:%s"]).unwrap_or_default();
    let branch = git_output(repo, &["branch", "--show-current"]).unwrap_or_default();
    let diff = truncate_chars(&diff, 40_000);

    Ok(format!(
        r#"Write a useful git commit message for the {scope}.

Rules:
- Return only the complete commit message.
- No markdown, no explanation, no surrounding quotes.
- First line should be 72 characters or less.
- If the change is more than trivial, add a blank line followed by a concise body.
- In the body, explain what changed and why it matters for the current branch.
- Mention important behavior, UI, data, or workflow impact visible in the diff.
- Do not invent tests or ticket numbers.
- Use the existing repository style from recent commits when it is clear.
- For tiny changes, a subject-only message is acceptable.

Current branch:
{branch}

Recent commit subjects:
{history}

Diff stat:
{stat}

Diff:
{diff}
"#
    ))
}

fn format_file_diffs_for_generation(diffs: &[FileDiff]) -> String {
    let mut out = String::new();
    for diff in diffs {
        out.push_str("diff -- ");
        out.push_str(&diff.path.display().to_string());
        out.push('\n');
        if diff.is_binary {
            out.push_str("Binary file changed\n\n");
            continue;
        }
        for hunk in &diff.hunks {
            out.push_str(&hunk.header);
            out.push('\n');
            for line in &hunk.lines {
                let prefix = match line.origin {
                    ide_core::git::LineOrigin::Add => '+',
                    ide_core::git::LineOrigin::Remove => '-',
                    ide_core::git::LineOrigin::Context => ' ',
                };
                out.push(prefix);
                out.push_str(&line.text);
                out.push('\n');
            }
        }
        out.push('\n');
    }
    out
}

fn format_selected_diff_stat(diffs: &[FileDiff]) -> String {
    let mut total_additions = 0;
    let mut total_removals = 0;
    let file_stats = diffs
        .iter()
        .map(|diff| {
            let additions = diff
                .hunks
                .iter()
                .flat_map(|hunk| &hunk.lines)
                .filter(|line| line.origin == ide_core::git::LineOrigin::Add)
                .count();
            let removals = diff
                .hunks
                .iter()
                .flat_map(|hunk| &hunk.lines)
                .filter(|line| line.origin == ide_core::git::LineOrigin::Remove)
                .count();
            total_additions += additions;
            total_removals += removals;
            format!("{} | +{} -{}", diff.path.display(), additions, removals)
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "{} files changed, +{} -{}\n{}",
        diffs.len(),
        total_additions,
        total_removals,
        file_stats
    )
}

fn selected_file_diffs(repo: &Path, files: &[PathBuf]) -> anyhow::Result<Vec<FileDiff>> {
    let selected = files
        .iter()
        .map(|path| normalize_generation_path(repo, path))
        .collect::<std::collections::HashSet<_>>();
    Ok(ide_core::git::worktree_diffs(repo)?
        .into_iter()
        .filter(|diff| selected.contains(&normalize_generation_path(repo, &diff.path)))
        .collect())
}

fn normalize_generation_path(repo: &Path, path: &Path) -> PathBuf {
    let relative = if path.is_absolute() {
        path.strip_prefix(repo).unwrap_or(path)
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

fn build_commit_prompt_for_files(repo: &Path, files: &[PathBuf]) -> anyhow::Result<String> {
    let diffs = selected_file_diffs(repo, files)?;
    if diffs.is_empty() {
        anyhow::bail!("No selected-file diff available for commit message generation");
    }
    let history = git_output(repo, &["log", "-8", "--pretty=format:%s"]).unwrap_or_default();
    let branch = git_output(repo, &["branch", "--show-current"]).unwrap_or_default();
    let stat = format_selected_diff_stat(&diffs);
    let diff = truncate_chars(&format_file_diffs_for_generation(&diffs), 40_000);

    Ok(format!(
        r#"Write a useful git commit message for these selected working-tree changes.

Rules:
- Return only the complete commit message.
- No markdown, no explanation, no surrounding quotes.
- First line should be 72 characters or less.
- If this is part of an existing branch or pull request, write the message as one commit in that larger branch context.
- If the change is more than trivial, add a blank line followed by a concise body.
- In the body, explain what changed, why it matters, and any important UI/workflow impact.
- Use concrete details from the selected files instead of generic text.
- Do not invent tests, issue IDs, or ticket numbers.
- Use the existing repository style from recent commits when it is clear.
- For tiny changes, a subject-only message is acceptable.

Current branch:
{branch}

Recent commit subjects:
{history}

Selected diff stat:
{stat}

Selected diff:
{diff}
"#
    ))
}

fn wait_with_timeout(
    child: &mut std::process::Child,
    timeout: Duration,
    provider: &str,
) -> anyhow::Result<std::process::ExitStatus> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("{provider} generation timed out");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn strip_ansi(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            output.push(ch);
        }
    }
    output
}

fn sanitize_commit_message(text: &str) -> anyhow::Result<String> {
    let mut text = strip_ansi(text).trim().to_string();
    if text.starts_with("```") {
        text = text
            .lines()
            .filter(|line| !line.trim_start().starts_with("```"))
            .collect::<Vec<_>>()
            .join("\n");
    }
    text = text
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim()
        .to_string();
    while text.contains("\n\n\n") {
        text = text.replace("\n\n\n", "\n\n");
    }
    if text.is_empty() {
        anyhow::bail!("The selected AI provider returned an empty commit message");
    }
    Ok(text)
}

pub(crate) fn generate_commit_message(
    generation_agent: &GenerationAgent,
    repo: &Path,
    use_staged: bool,
) -> anyhow::Result<String> {
    let prompt = build_commit_prompt(repo, use_staged)?;
    let output = run_streamed_generation(generation_agent, repo, prompt, Duration::from_secs(120))?;
    sanitize_commit_message(&output)
}

pub(crate) fn generate_commit_message_for_files(
    generation_agent: &GenerationAgent,
    repo: &Path,
    files: &[PathBuf],
) -> anyhow::Result<String> {
    let prompt = build_commit_prompt_for_files(repo, files)?;
    let output = run_streamed_generation(generation_agent, repo, prompt, Duration::from_secs(120))?;
    sanitize_commit_message(&output)
}

fn sanitize_riff_instructions(text: &str) -> anyhow::Result<String> {
    let mut text = strip_ansi(text).trim().to_string();
    if text.starts_with("```") {
        text = text
            .lines()
            .filter(|line| !line.trim_start().starts_with("```"))
            .collect::<Vec<_>>()
            .join("\n");
    }
    let text = text.trim().to_string();
    if text.is_empty() {
        anyhow::bail!("The selected AI provider returned empty Riff instructions");
    }
    Ok(text)
}

pub(crate) fn generate_riff(
    generation_agent: &GenerationAgent,
    working_directory: &Path,
    name: &str,
    description: &str,
) -> anyhow::Result<String> {
    let name = name.trim();
    let description = description.trim();
    if name.is_empty() || description.is_empty() {
        anyhow::bail!("A Riff name and description are required for generation");
    }

    let prompt = format!(
        r#"Write the reusable instruction body for an AI coding-agent capability called a Choro Riff.

Riff name: {name}
Riff description: {description}

Rules:
- Return only the instruction body. Do not add YAML frontmatter, a title, a markdown fence, or commentary.
- Write direct imperative instructions for the agent that will execute the Riff.
- Produce a concise, practical workflow rather than a persona or motivational prose.
- Tell the agent to inspect the current project and follow its conventions before acting.
- Include important decision points, edge cases, safety boundaries, and proportionate verification.
- Do not invent project-specific tools, commands, frameworks, paths, policies, or facts.
- Do not tell the agent to claim checks it did not perform.
- Keep the result broadly reusable across projects and coding agents.
"#
    );
    let output = run_streamed_generation(
        generation_agent,
        working_directory,
        prompt,
        Duration::from_secs(120),
    )?;
    sanitize_riff_instructions(&output)
}

/// A candidate memory distilled from one user decision, plus the scope the
/// model suggests. `Ok(None)` from [`distill_memory_proposal`] means "nothing
/// worth remembering" — malformed output is folded into that, never an error.
pub(crate) struct DistilledMemoryProposal {
    pub(crate) text: String,
    pub(crate) global: bool,
    pub(crate) why: String,
}

/// The decision being examined for a durable preference.
pub(crate) enum MemoryDecisionContext {
    PlanFeedback {
        feedback: String,
        plan_markdown: String,
    },
    /// (question, the user's free-text answer) pairs.
    QuestionAnswers { pairs: Vec<(String, String)> },
    /// Agent-authored living summary. This is weaker evidence than direct user
    /// text, so the distiller may only extract an explicitly documented user
    /// preference and must ignore task facts and implementation decisions.
    AgentSummary { summary: String },
}

const MEMORY_PROPOSAL_PLAN_BUDGET: usize = 4_000;
const MEMORY_PROPOSAL_ANSWER_BUDGET: usize = 1_000;
const MEMORY_PROPOSAL_KNOWN_BUDGET: usize = 8_000;

pub(crate) fn distill_memory_proposal(
    generation_agent: &GenerationAgent,
    working_directory: &Path,
    project_name: &str,
    decision: &MemoryDecisionContext,
    known_memories: &[String],
) -> anyhow::Result<Option<DistilledMemoryProposal>> {
    let decision_block = match decision {
        MemoryDecisionContext::PlanFeedback {
            feedback,
            plan_markdown,
        } => format!(
            "The assistant proposed this plan:\n---\n{}\n---\nThe developer replied with this correction:\n\"{}\"",
            bounded_chars(plan_markdown, MEMORY_PROPOSAL_PLAN_BUDGET),
            bounded_chars(feedback, MEMORY_PROPOSAL_ANSWER_BUDGET),
        ),
        MemoryDecisionContext::QuestionAnswers { pairs } => pairs
            .iter()
            .map(|(question, answer)| {
                format!(
                    "The assistant asked: \"{}\"\nThe developer answered in their own words: \"{}\"",
                    bounded_chars(question, MEMORY_PROPOSAL_ANSWER_BUDGET),
                    bounded_chars(answer, MEMORY_PROPOSAL_ANSWER_BUDGET),
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n"),
        MemoryDecisionContext::AgentSummary { summary } => format!(
            "The agent produced this living summary of its work:\n---\n{}\n---\nOnly treat a statement as preference evidence when the summary clearly attributes a reusable convention or correction to the developer. Task facts, code decisions, files, bugs, and outcomes are not preferences.",
            bounded_chars(summary, MEMORY_PROPOSAL_PLAN_BUDGET),
        ),
    };
    let mut known_block = String::new();
    for memory in known_memories {
        let line = format!(
            "- {}\n",
            bounded_chars(memory, ide_core::local_store::MAX_MEMORY_TEXT_CHARS)
        );
        if known_block.len() + line.len() > MEMORY_PROPOSAL_KNOWN_BUDGET {
            break;
        }
        known_block.push_str(&line);
    }
    if known_block.is_empty() {
        known_block.push_str("(none yet)\n");
    }

    let prompt = format!(
        r#"You review one decision a developer just made in the "{project_name}" project and decide whether it reveals ONE durable, generalizable preference worth remembering for future AI coding sessions.

The decision:
{decision_block}

Preferences already remembered (never re-propose these or close variants):
{known_block}
Rules:
- Default to NONE. Only extract a preference that is clearly reusable in FUTURE, unrelated tasks — not a one-off instruction about this task, file, or bug.
- The rule must be one short imperative sentence under 300 characters, self-contained, with no references to "this plan", "this task", or the conversation.
- SCOPE is "global" only when the preference is clearly independent of this codebase (for example commit-message style, reply language, general workflow taste). When in doubt, use "project".
- Respond with EXACTLY one of:
NONE
or:
TEXT: <the rule>
SCOPE: project|global
WHY: <one short sentence pointing at what the developer said that shows this>
No other output."#
    );
    let output = run_streamed_generation(
        generation_agent,
        working_directory,
        prompt,
        Duration::from_secs(120),
    )?;
    Ok(parse_memory_proposal_response(&output))
}

fn bounded_chars(text: &str, max_chars: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let bounded: String = text.chars().take(max_chars).collect();
    format!("{}…", bounded.trim_end())
}

fn parse_memory_proposal_response(text: &str) -> Option<DistilledMemoryProposal> {
    let cleaned = strip_ansi(text);
    let mut lines = cleaned
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("```"))
        .peekable();
    let first = lines.peek()?;
    if first.eq_ignore_ascii_case("none") {
        return None;
    }
    let mut rule_text = None;
    let mut scope = None;
    let mut why = String::new();
    for line in lines {
        if let Some(value) = line.strip_prefix("TEXT:") {
            rule_text = Some(value.trim().to_string());
        } else if let Some(value) = line.strip_prefix("SCOPE:") {
            scope = Some(value.trim().to_ascii_lowercase());
        } else if let Some(value) = line.strip_prefix("WHY:") {
            why = value.trim().to_string();
        }
    }
    let text = rule_text?.trim().trim_matches('"').trim().to_string();
    if text.is_empty() || text.chars().count() > ide_core::local_store::MAX_MEMORY_TEXT_CHARS {
        return None;
    }
    let global = match scope?.as_str() {
        "global" => true,
        "project" => false,
        _ => return None,
    };
    Some(DistilledMemoryProposal { text, global, why })
}

#[derive(serde::Deserialize)]
pub(crate) struct GeneratedPullRequest {
    pub(crate) title: String,
    pub(crate) body: String,
}

pub(crate) fn default_remote_branch(repo: &Path) -> String {
    git_output(
        repo,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    )
    .ok()
    .and_then(|head| head.trim().strip_prefix("origin/").map(str::to_string))
    .filter(|branch| !branch.is_empty())
    .unwrap_or_else(|| "main".to_string())
}

pub(crate) fn pull_request_base_branch_options(
    default_branch: &str,
    branches: &[BranchInfo],
) -> Vec<String> {
    fn push_unique(options: &mut Vec<String>, branch: impl Into<String>) {
        let branch = branch.into();
        if !branch.is_empty() && !options.iter().any(|option| option == &branch) {
            options.push(branch);
        }
    }

    let mut options = Vec::new();
    push_unique(&mut options, default_branch.trim());

    for branch in branches.iter().filter(|branch| branch.is_remote) {
        let name = branch
            .name
            .strip_prefix("origin/")
            .unwrap_or(branch.name.as_str());
        if name != "HEAD" {
            push_unique(&mut options, name);
        }
    }

    for branch in branches.iter().filter(|branch| !branch.is_remote) {
        push_unique(&mut options, branch.name.as_str());
    }

    if options.is_empty() {
        options.push("main".to_string());
    }

    options
}

fn read_pull_request_template(repo: &Path) -> Option<String> {
    [
        ".github/pull_request_template.md",
        ".github/PULL_REQUEST_TEMPLATE.md",
        "PULL_REQUEST_TEMPLATE.md",
    ]
    .iter()
    .map(|path| repo.join(path))
    .find_map(|path| std::fs::read_to_string(path).ok())
    .filter(|template| !template.trim().is_empty())
}

fn build_pull_request_prompt(
    repo: &Path,
    branch: &str,
    base_branch: &str,
) -> anyhow::Result<String> {
    let base_branch = base_branch.trim();
    if base_branch.is_empty() {
        anyhow::bail!("Choose a base branch for the pull request");
    }
    let base_ref = format!("origin/{base_branch}");
    let rev = format!("{base_ref}..HEAD");
    let diff_rev = format!("{base_ref}...HEAD");
    let commits = git_output(repo, &["log", "--pretty=format:%h %s", &rev]).unwrap_or_default();
    let stat = git_output(repo, &["diff", "--stat", &diff_rev]).unwrap_or_default();
    let diff = git_output(repo, &["diff", &diff_rev, "--"]).unwrap_or_default();
    let recent = git_output(repo, &["log", "-8", "--pretty=format:%s"]).unwrap_or_default();
    let template = read_pull_request_template(repo)
        .unwrap_or_else(|| "## Summary\n- \n\n## Testing\n- ".to_string());
    let diff = truncate_diff_across_files(&diff, PULL_REQUEST_DIFF_CHAR_LIMIT);

    if commits.trim().is_empty() && diff.trim().is_empty() {
        anyhow::bail!("No branch changes found for pull request text");
    }

    Ok(format!(
        r#"Write a GitHub pull request title and body for this branch.

Rules:
- Return only JSON with keys "title" and "body".
- No markdown fence and no explanation outside the JSON.
- Title should be concise, 72 characters or less.
- Body must follow the provided pull request structure, but may add useful subsections within it.
- Match the body's depth to the size and complexity of the change. A tiny focused change can be brief. A large or multi-area change needs an executive summary plus concrete details grouped by meaningful subsystem, feature, or workflow.
- Use the commit list, diff stat, and diff to cover every material theme. Do not reduce a large change to a few generic bullets.
- Explain what behavior changed, why it matters to users or maintainers, and how the important pieces work together. Prefer a reviewer-oriented narrative over a file-by-file inventory.
- Call out migrations, persistence or data-model changes, configuration, compatibility concerns, operational impact, and notable risks when the supplied evidence supports them.
- Do not invent motivation, behavior, risks, tests, issue IDs, or implementation details that are not supported by the supplied material.
- Keep each section focused and omit filler; detail should earn its place rather than satisfy a fixed length.
- If testing is unknown, say "Not run".

Branch:
{branch}

Base:
{base_ref}

Pull request structure:
{template}

Recent commit subjects for style:
{recent}

Commits on this branch:
{commits}

Diff stat:
{stat}

Diff:
{diff}
"#
    ))
}

fn build_pull_request_prompt_for_files(
    repo: &Path,
    branch: &str,
    base_branch: &str,
    files: &[PathBuf],
    commit_message: &str,
) -> anyhow::Result<String> {
    let base_branch = base_branch.trim();
    if base_branch.is_empty() {
        anyhow::bail!("Choose a base branch for the pull request");
    }
    let base_ref = format!("origin/{base_branch}");
    let diffs = selected_file_diffs(repo, files)?;
    if diffs.is_empty() {
        anyhow::bail!("No selected-file diff available for pull request text");
    }
    let recent = git_output(repo, &["log", "-8", "--pretty=format:%s"]).unwrap_or_default();
    let template = read_pull_request_template(repo)
        .unwrap_or_else(|| "## Summary\n- \n\n## Testing\n- ".to_string());
    let stat = format_selected_diff_stat(&diffs);
    let diff = truncate_diff_across_files(
        &format_file_diffs_for_generation(&diffs),
        PULL_REQUEST_DIFF_CHAR_LIMIT,
    );

    Ok(format!(
        r#"Write a GitHub pull request title and body for these selected working-tree changes.

Rules:
- Return only JSON with keys "title" and "body".
- No markdown fence and no explanation outside the JSON.
- Title should be concise, 72 characters or less.
- Body must follow the provided pull request structure, but may add useful subsections within it.
- Match the body's depth to the size and complexity of the change. A tiny focused change can be brief. A large or multi-area change needs an executive summary plus concrete details grouped by meaningful subsystem, feature, or workflow.
- Use the proposed commit message, diff stat, and diff to cover every material theme. Do not reduce a large change to a few generic bullets.
- Explain what behavior changed, why it matters to users or maintainers, and how the important pieces work together. Prefer a reviewer-oriented narrative over a file-by-file inventory.
- Call out migrations, persistence or data-model changes, configuration, compatibility concerns, operational impact, and notable risks when the supplied evidence supports them.
- Do not invent motivation, behavior, risks, tests, issue IDs, or implementation details that are not supported by the supplied material.
- Keep each section focused and omit filler; detail should earn its place rather than satisfy a fixed length.
- If testing is unknown, say "Not run".

Branch:
{branch}

Base:
{base_ref}

Proposed commit message:
{commit_message}

Pull request structure:
{template}

Recent commit subjects for style:
{recent}

Selected diff stat:
{stat}

Selected diff:
{diff}
"#
    ))
}

fn sanitize_pull_request_response(text: &str) -> anyhow::Result<GeneratedPullRequest> {
    let mut text = strip_ansi(text).trim().to_string();
    if text.starts_with("```") {
        text = text
            .lines()
            .filter(|line| !line.trim_start().starts_with("```"))
            .collect::<Vec<_>>()
            .join("\n");
    }
    let start = text.find('{').ok_or_else(|| {
        anyhow::anyhow!("The selected AI provider did not return pull request JSON")
    })?;
    let end = text.rfind('}').ok_or_else(|| {
        anyhow::anyhow!("The selected AI provider did not return pull request JSON")
    })?;
    let json = &text[start..=end];
    let pull_request: GeneratedPullRequest = serde_json::from_str(json)?;
    if pull_request.title.trim().is_empty() || pull_request.body.trim().is_empty() {
        anyhow::bail!("The selected AI provider returned an empty pull request title or body");
    }
    Ok(GeneratedPullRequest {
        title: pull_request.title.trim().to_string(),
        body: pull_request.body.trim().to_string(),
    })
}

pub(crate) fn generate_pull_request(
    generation_agent: &GenerationAgent,
    repo: &Path,
    branch: &str,
    base_branch: &str,
) -> anyhow::Result<GeneratedPullRequest> {
    let prompt = build_pull_request_prompt(repo, branch, base_branch)?;
    let output = run_streamed_generation(generation_agent, repo, prompt, Duration::from_secs(180))?;
    sanitize_pull_request_response(&output)
}

pub(crate) fn generate_pull_request_for_files(
    generation_agent: &GenerationAgent,
    repo: &Path,
    branch: &str,
    base_branch: &str,
    files: &[PathBuf],
    commit_message: &str,
) -> anyhow::Result<GeneratedPullRequest> {
    let prompt =
        build_pull_request_prompt_for_files(repo, branch, base_branch, files, commit_message)?;
    let output = run_streamed_generation(generation_agent, repo, prompt, Duration::from_secs(180))?;
    sanitize_pull_request_response(&output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_pr_diffs_keep_a_sample_from_every_changed_file() {
        let diff = (1..=4)
            .map(|index| {
                format!(
                    "diff --git a/file-{index}.rs b/file-{index}.rs\n{}",
                    format!("+change-{index}\n").repeat(50)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        let sampled = truncate_diff_across_files(&diff, 400);

        for index in 1..=4 {
            assert!(sampled.contains(&format!("diff --git a/file-{index}.rs")));
        }
        assert!(sampled.contains("[large diff sampled across every changed file]"));
        assert!(sampled.len() < diff.len());
    }

    #[test]
    fn memory_proposal_parser_accepts_none_and_valid_shapes() {
        assert!(parse_memory_proposal_response("NONE").is_none());
        assert!(parse_memory_proposal_response("\n  none \n").is_none());

        let project = parse_memory_proposal_response(
            "TEXT: Use the seeded local database, never prod.\nSCOPE: project\nWHY: You denied a prod migration.",
        )
        .expect("project proposal");
        assert!(!project.global);
        assert_eq!(project.text, "Use the seeded local database, never prod.");
        assert_eq!(project.why, "You denied a prod migration.");

        let global = parse_memory_proposal_response(
            "```\nTEXT: Write commit messages in English.\nSCOPE: global\nWHY: You rewrote one.\n```",
        )
        .expect("fenced global proposal");
        assert!(global.global);
    }

    #[test]
    fn memory_proposal_parser_drops_malformed_output() {
        // Missing SCOPE.
        assert!(parse_memory_proposal_response("TEXT: A rule.\nWHY: reason").is_none());
        // Unknown scope value.
        assert!(parse_memory_proposal_response("TEXT: A rule.\nSCOPE: everywhere").is_none());
        // Over the memory length cap.
        let long = format!("TEXT: {}\nSCOPE: project", "x".repeat(600));
        assert!(parse_memory_proposal_response(&long).is_none());
        // Chatty output with no contract lines.
        assert!(parse_memory_proposal_response("Sure! Here's a rule you could save.").is_none());
    }

    #[test]
    fn open_code_text_generation_denies_every_tool() {
        let config: serde_json::Value =
            serde_json::from_str(OPENCODE_TEXT_GENERATION_CONFIG).unwrap();
        assert_eq!(
            config.pointer("/permission/*"),
            Some(&serde_json::json!("deny"))
        );
        assert_eq!(
            config.pointer("/agent/choro-text-generation/permission/*"),
            Some(&serde_json::json!("deny"))
        );
        assert_eq!(config.get("share"), Some(&serde_json::json!("disabled")));
        assert_eq!(config.get("snapshot"), Some(&serde_json::json!(false)));
    }

    #[test]
    fn quick_ask_provider_permissions_allow_investigation_without_mutation() {
        assert!(CODEX_QUICK_ASK_PERMISSION_CONFIG
            .contains(&"permissions.quick-ask.extends=\":read-only\""));
        assert!(CODEX_QUICK_ASK_PERMISSION_CONFIG
            .contains(&"permissions.quick-ask.network.enabled=true"));

        let config: serde_json::Value = serde_json::from_str(OPENCODE_QUICK_ASK_CONFIG).unwrap();
        let permissions = config.pointer("/agent/choro-quick-ask/permission").unwrap();
        assert_eq!(permissions.get("*"), Some(&serde_json::json!("deny")));
        assert_eq!(permissions.get("read"), Some(&serde_json::json!("allow")));
        assert_eq!(
            permissions.get("webfetch"),
            Some(&serde_json::json!("allow"))
        );
        assert_eq!(
            permissions.pointer("/bash/*"),
            Some(&serde_json::json!("deny"))
        );
        assert_eq!(
            permissions.pointer("/bash/git status*"),
            Some(&serde_json::json!("allow"))
        );
        assert_eq!(permissions.pointer("/bash/git push*"), None);
        assert_eq!(permissions.get("edit"), None);
    }

    #[test]
    fn open_code_generation_output_contains_only_completed_text() {
        let output = concat!(
            "{\"type\":\"step_start\",\"sessionID\":\"ses_123\"}\n",
            "{\"type\":\"text\",\"sessionID\":\"ses_123\",\"part\":{\"text\":\"Generated title\"}}\n",
            "{\"type\":\"step_finish\",\"sessionID\":\"ses_123\"}\n"
        );
        assert_eq!(
            parse_open_code_generation_output(output),
            (Some("ses_123".to_string()), "Generated title".to_string())
        );
    }

    #[test]
    fn claude_multimodal_input_contains_the_prompt_and_image_payload() {
        let image = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("assets/app-icon/app-icon-demo-1024.png");
        let images = validate_generation_images(&[image]).expect("valid bundled image");
        let input = claude_stream_json_input("Describe this image", &images).unwrap();
        let event: serde_json::Value = serde_json::from_str(input.trim()).unwrap();
        let content = event
            .pointer("/message/content")
            .and_then(serde_json::Value::as_array)
            .unwrap();

        assert_eq!(
            content[0].get("text").and_then(|value| value.as_str()),
            Some("Describe this image")
        );
        assert_eq!(
            content[1].pointer("/source/media_type"),
            Some(&serde_json::json!("image/png"))
        );
        assert!(content[1]
            .pointer("/source/data")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|data| !data.is_empty()));
    }
}
