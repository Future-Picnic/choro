use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{bail, Context as _, Result};
use serde_json::Value;

use super::agent_chat::protocol::{agent_command_path_env, find_opencode_executable};

pub const OPEN_CODE_CATALOG_STALE_AFTER: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenCodeModel {
    /// Provider-qualified id accepted by OpenCode, such as `openai/gpt-5.4`.
    pub id: String,
    pub provider_id: String,
    pub name: String,
    pub variants: Vec<String>,
    pub free: bool,
}

impl OpenCodeModel {
    pub fn provider_label(&self) -> String {
        provider_label(&self.provider_id)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OpenCodeCatalogState {
    #[default]
    NotLoaded,
    Loading,
    Ready,
    NotInstalled,
    Failed,
}

#[derive(Clone, Debug)]
pub struct OpenCodeCatalog {
    pub state: OpenCodeCatalogState,
    pub project_path: Option<PathBuf>,
    pub executable: Option<PathBuf>,
    pub models: Vec<OpenCodeModel>,
    pub error: Option<String>,
    pub refreshed_at: Option<Instant>,
}

impl Default for OpenCodeCatalog {
    fn default() -> Self {
        Self {
            state: OpenCodeCatalogState::NotLoaded,
            project_path: None,
            executable: None,
            models: Vec::new(),
            error: None,
            refreshed_at: None,
        }
    }
}

impl OpenCodeCatalog {
    pub fn is_stale_for(&self, project_path: &Path) -> bool {
        self.project_path.as_deref() != Some(project_path)
            || self
                .refreshed_at
                .is_none_or(|refreshed| refreshed.elapsed() >= OPEN_CODE_CATALOG_STALE_AFTER)
    }
}

#[derive(Debug)]
pub struct OpenCodeDiscovery {
    pub executable: PathBuf,
    pub models: Vec<OpenCodeModel>,
}

pub fn discover_open_code_models(project_path: &Path) -> Result<Option<OpenCodeDiscovery>> {
    let Some(executable) = find_opencode_executable() else {
        return Ok(None);
    };
    let output = Command::new(&executable)
        .args(["models", "--verbose"])
        .current_dir(project_path)
        .env("PATH", agent_command_path_env())
        .output()
        .with_context(|| format!("failed to start {}", executable.display()))?;
    if !output.status.success() {
        let detail =
            ide_core::redact_sensitive_text(String::from_utf8_lossy(&output.stderr).trim());
        if detail.is_empty() {
            bail!("OpenCode model discovery exited with {}", output.status);
        }
        bail!("OpenCode model discovery failed: {detail}");
    }
    let stdout = String::from_utf8(output.stdout).context("OpenCode returned non-UTF-8 output")?;
    let models = parse_verbose_models(&stdout)?;
    Ok(Some(OpenCodeDiscovery { executable, models }))
}

pub fn parse_verbose_models(output: &str) -> Result<Vec<OpenCodeModel>> {
    let bytes = output.as_bytes();
    let mut cursor = 0usize;
    let mut models = Vec::new();

    while cursor < bytes.len() {
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor >= bytes.len() {
            break;
        }
        let line_end = bytes[cursor..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|offset| cursor + offset)
            .unwrap_or(bytes.len());
        let listed_id = output[cursor..line_end].trim();
        cursor = (line_end + 1).min(bytes.len());
        if !listed_id.contains('/') {
            continue;
        }
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        let mut stream =
            serde_json::Deserializer::from_slice(&bytes[cursor..]).into_iter::<Value>();
        let metadata = stream
            .next()
            .transpose()
            .with_context(|| format!("invalid OpenCode metadata for {listed_id}"))?
            .with_context(|| format!("missing OpenCode metadata for {listed_id}"))?;
        cursor += stream.byte_offset();

        let provider_id = metadata
            .get("providerID")
            .and_then(Value::as_str)
            .or_else(|| listed_id.split_once('/').map(|(provider, _)| provider))
            .unwrap_or("unknown")
            .to_string();
        let name = metadata
            .get("name")
            .and_then(Value::as_str)
            .or_else(|| listed_id.split_once('/').map(|(_, model)| model))
            .unwrap_or(listed_id)
            .to_string();
        let mut variants = metadata
            .get("variants")
            .and_then(Value::as_object)
            .map(|variants| variants.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        variants.sort();
        let free = metadata
            .get("cost")
            .and_then(Value::as_object)
            .is_some_and(|cost| {
                ["input", "output"]
                    .into_iter()
                    .all(|key| cost.get(key).and_then(Value::as_f64) == Some(0.))
            });
        models.push(OpenCodeModel {
            id: listed_id.to_string(),
            provider_id,
            name,
            variants,
            free,
        });
    }

    models.sort_by(|left, right| {
        let left_zen = left.provider_id != "opencode";
        let right_zen = right.provider_id != "opencode";
        (left_zen, left.provider_label(), left.name.to_lowercase()).cmp(&(
            right_zen,
            right.provider_label(),
            right.name.to_lowercase(),
        ))
    });
    Ok(models)
}

fn provider_label(provider_id: &str) -> String {
    match provider_id {
        "opencode" => "OpenCode Zen".to_string(),
        "openai" => "OpenAI".to_string(),
        "anthropic" => "Anthropic".to_string(),
        "google" => "Google".to_string(),
        "openrouter" => "OpenRouter".to_string(),
        "github-copilot" => "GitHub Copilot".to_string(),
        "amazon-bedrock" => "Amazon Bedrock".to_string(),
        "azure" => "Azure OpenAI".to_string(),
        "xai" => "xAI".to_string(),
        other => other
            .split(['-', '_'])
            .filter(|part| !part.is_empty())
            .map(|part| {
                let mut chars = part.chars();
                chars
                    .next()
                    .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_provider_qualified_verbose_catalog() {
        let models = parse_verbose_models(
            r#"opencode/big-pickle
{
  "id": "big-pickle",
  "providerID": "opencode",
  "name": "Big Pickle",
  "cost": { "input": 0, "output": 0 },
  "variants": {}
}
openai/gpt-5.4
{
  "id": "gpt-5.4",
  "providerID": "openai",
  "name": "GPT-5.4",
  "cost": { "input": 1.25, "output": 10 },
  "variants": { "high": {}, "low": {} }
}
"#,
        )
        .unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "opencode/big-pickle");
        assert_eq!(models[0].provider_label(), "OpenCode Zen");
        assert!(models[0].free);
        assert!(models[0].variants.is_empty());
        assert_eq!(models[1].provider_label(), "OpenAI");
        assert!(!models[1].free);
        assert_eq!(models[1].variants, ["high", "low"]);
    }
}
