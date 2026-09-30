use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::agents::{self, shell_quote, AgentAccessMode, AgentEffort, AgentKind, AgentModel};
use crate::branding::migrate_legacy_doc_path;
use crate::{AppConfig, ProjectId};

const DOC_ASSISTANTS_SCHEMA_VERSION: u32 = 1;
pub const DOC_PROPOSAL_START: &str = "DOC_PROPOSAL";
pub const DOC_PROPOSAL_END: &str = "END_DOC_PROPOSAL";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocAssistantRecord {
    #[serde(default = "uuid::Uuid::new_v4")]
    pub chat_agent_id: uuid::Uuid,
    pub project_id: ProjectId,
    pub relative_doc_path: PathBuf,
    pub provider: AgentKind,
    pub model: AgentModel,
    /// Provider-qualified OpenCode model id, for example `openai/gpt-5.4`.
    #[serde(default)]
    pub external_model_id: Option<String>,
    /// Human-readable OpenCode model name captured at selection time.
    #[serde(default)]
    pub external_model_label: Option<String>,
    /// Model-specific reasoning variants reported by OpenCode.
    #[serde(default)]
    pub external_model_variants: Vec<String>,
    pub effort: AgentEffort,
    #[serde(default)]
    pub access_mode: AgentAccessMode,
    #[serde(default)]
    pub chat_session_id: Option<String>,
    #[serde(default)]
    pub cli_session_id: Option<String>,
    #[serde(default)]
    pub last_transcript_path: Option<PathBuf>,
    #[serde(default)]
    pub pending_proposal: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl DocAssistantRecord {
    pub fn new(project_id: ProjectId, relative_doc_path: PathBuf) -> Self {
        let now = agents::unix_now();
        Self {
            chat_agent_id: uuid::Uuid::new_v4(),
            project_id,
            relative_doc_path,
            provider: AgentKind::Codex,
            model: AgentModel::default_for(AgentKind::Codex),
            external_model_id: None,
            external_model_label: None,
            external_model_variants: Vec::new(),
            effort: AgentEffort::Medium,
            access_mode: AgentAccessMode::FullAccess,
            chat_session_id: None,
            cli_session_id: None,
            last_transcript_path: None,
            pending_proposal: None,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn key(&self) -> String {
        doc_assistant_key(self.project_id, &self.relative_doc_path)
    }

    pub fn title(&self) -> String {
        self.relative_doc_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map(|stem| format!("Docs: {}", stem.replace('-', " ")))
            .unwrap_or_else(|| "Docs Assistant".to_string())
    }

    pub fn start_command(&self, prompt: &str) -> String {
        start_command(self, prompt)
    }

    pub fn resume_command(&self) -> Option<String> {
        let session_id = self.cli_session_id.as_deref()?;
        Some(resume_command(self.provider, self.access_mode, session_id))
    }

    pub fn model_cli_value(&self) -> Option<&str> {
        if self.provider == AgentKind::OpenCode {
            self.external_model_id.as_deref()
        } else {
            self.model.cli_value()
        }
    }

    pub fn supported_efforts(&self) -> Vec<AgentEffort> {
        if self.provider == AgentKind::OpenCode {
            AgentEffort::supported_variants(&self.external_model_variants)
        } else {
            self.model.efforts()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocAssistantStoreFile {
    pub version: u32,
    #[serde(default)]
    pub assistants: Vec<DocAssistantRecord>,
}

impl Default for DocAssistantStoreFile {
    fn default() -> Self {
        Self {
            version: DOC_ASSISTANTS_SCHEMA_VERSION,
            assistants: Vec::new(),
        }
    }
}

impl DocAssistantStoreFile {
    pub fn new(assistants: Vec<DocAssistantRecord>) -> Self {
        Self {
            version: DOC_ASSISTANTS_SCHEMA_VERSION,
            assistants,
        }
    }

    pub fn path() -> PathBuf {
        AppConfig::config_path()
            .parent()
            .map(|dir| dir.join("doc_assistants.json"))
            .unwrap_or_else(|| PathBuf::from("doc_assistants.json"))
    }

    pub fn load() -> Self {
        Self::load_from(&Self::path())
    }

    pub fn load_from(path: &Path) -> Self {
        match fs::read_to_string(path) {
            Ok(text) => {
                let mut store = serde_json::from_str::<Self>(&text).unwrap_or_default();
                if store.migrate_legacy_records() {
                    let _ = store.save_to(path);
                }
                store
            }
            Err(_) => Self::default(),
        }
    }

    fn migrate_legacy_records(&mut self) -> bool {
        let mut changed = false;
        for assistant in &mut self.assistants {
            let migrated = migrate_legacy_doc_path(&assistant.relative_doc_path);
            if migrated != assistant.relative_doc_path {
                assistant.relative_doc_path = migrated;
                changed = true;
            }
            if assistant.model == AgentModel::CodexDefault {
                assistant.model = AgentModel::default_for(AgentKind::Codex);
                assistant.effort = assistant.model.normalize_effort(assistant.effort);
                changed = true;
            }
        }
        changed
    }

    pub fn save(&self) -> Result<()> {
        self.save_to(&Self::path())
    }

    pub fn save_to(&self, path: &Path) -> Result<()> {
        let dir = path
            .parent()
            .context("doc assistants path has no parent directory")?;
        fs::create_dir_all(dir).context("failed to create doc assistants directory")?;
        let json =
            serde_json::to_string_pretty(self).context("failed to serialize doc assistants")?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, json).context("failed to write temp doc assistants file")?;
        fs::rename(&tmp, path).context("failed to move doc assistants file into place")?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocAssistantRole {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocAssistantMessage {
    pub role: DocAssistantRole,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocAssistantTranscriptMessage {
    pub role: DocAssistantRole,
    pub text: String,
    pub backend_message_id: Option<String>,
}

pub fn doc_assistant_key(project_id: ProjectId, relative_doc_path: &Path) -> String {
    format!(
        "{}:{}",
        project_id.0,
        relative_doc_path.to_string_lossy().replace('\\', "/")
    )
}

pub fn system_prompt(relative_doc_path: &Path) -> String {
    format!(
        "You are the Docs assistant for this project document: {}.\n\
    \n\
    You are not an implementation agent. Your job is to help the user think, structure, and write this one project document as a product/spec/research/design planning workspace before implementation starts.\n\
    \n\
    Default behavior:\n\
    - Be a conversational assistant first. Discuss, explain, compare options, ask clarifying questions, and help the user decide before changing the document.\n\
    - Do not edit the document just because the topic is in the doc. Treat wording like \"let's talk\", \"what do you think\", \"I'm not sure\", \"help me decide\", \"why\", \"explain\", \"brainstorm\", \"compare\", or \"options\" as discussion-only unless the user also explicitly asks you to update the doc.\n\
    - Edit only when the user clearly asks to add, update, rewrite, draft, improve, apply, or put content in the document. If the request is ambiguous, ask before editing.\n\
    - When discussing, answer in chat and do not modify the file. You may offer a concise suggested doc change, but wait for approval before applying it.\n\
    \n\
    Document naming on the first edit:\n\
    - When you edit the document for the first time, check whether its filename is still the generated placeholder `untitled.choro` or `untitled-N.choro`.\n\
    - If and only if the filename is still that placeholder, replace the document JSON's `title` with a concise, meaningful title that describes what the document is actually about. Do this as part of the same edit, even when the template currently has a generic title such as `Feature / PRD`.\n\
    - Do not rename or move the file yourself; Choro will rename it safely from the updated JSON title after your turn finishes.\n\
    - If the filename is not an untitled placeholder, the user has already named the document. Preserve its title exactly unless the user explicitly asks to rename it.\n\
    \n\
    Project grounding is mandatory:\n\
    - Treat this document as part of the repository, never as an isolated or generic writing exercise.\n\
    - Before your first substantive response, and before every requested document edit, inspect the current document and enough of the repository to understand the actual project. Do this before drafting conclusions or prose.\n\
    - Start with project instructions and orientation files when present (for example AGENTS.md, README files, manifests, and relevant existing docs), then search for and read the source code, configuration, tests, schemas, and UI involved in the user's topic. Follow references until the claims you plan to make are grounded in the implementation.\n\
    - Do not rely on a remembered framework pattern or a generic product template when repository evidence is available. Never imply that you inspected code you did not read.\n\
    - Scale the inspection to the request, but for project-specific behavior always inspect the relevant implementation. If the request spans multiple systems, inspect each affected area.\n\
    - Briefly name the concrete project files or implementation facts that informed your answer. If the relevant files cannot be found or read, say that clearly and ask for the missing context instead of filling gaps with generic assumptions.\n\
    - Re-check the repository when the topic changes or when earlier findings may be stale; do not assume that reading the document once is sufficient for the whole conversation.\n\
    \n\
    Hard boundaries:\n\
    - You may read the entire project whenever needed for grounding. Reading is expected; the write boundary below does not limit repository inspection.\n\
    - When explicitly asked to edit, you may edit this document directly: {}.\n\
    - This document is the only file you may write. Do not create, edit, rename, delete, or format any other file.\n\
    - You must not implement code, change source files, modify project configuration, install packages, run migrations, or execute project tasks.\n\
    - If the user asks you to build, fix, refactor, execute, or modify files, translate that request into a clear doc/spec/plan instead.\n\
    - Your only durable artifact, when the user explicitly asks for one, is the content of this one doc.\n\
    \n\
    Document format:\n\
    - The `.choro` document is JSON with `version: 1`, `format: \"blocknote\"`, a title, and a `blocks` array.\n\
    - Preserve valid JSON and never change `version` or `format`.\n\
    - Edit the `blocks` array directly. Common block types are `paragraph`, `heading` (with `props.level` 1-3), `bulletListItem`, `numberedListItem`, `checkListItem`, `quote`, and `codeBlock`.\n\
    - A simple block can use a string `content`; rich inline content is an array of text objects. Preserve block IDs and unknown props when editing existing blocks.\n\
    - Images and other structured blocks must be preserved unless the user explicitly asks to remove them.\n\
    \n\
    When making changes, edit only the Choro JSON document above directly. Do not make changes during exploratory discussion.\n\
    \n\
    If the request is unclear, ask concise clarifying questions. Keep responses focused on improving the document and decisions needed before implementation.",
        relative_doc_path.to_string_lossy(),
        relative_doc_path.to_string_lossy()
    )
}

pub fn user_prompt(relative_doc_path: &Path, message: &str) -> String {
    format!(
        "{}\n\nUser request:\n{}",
        system_prompt(relative_doc_path),
        message.trim()
    )
}

pub fn quick_action_task(action: &str) -> String {
    match action {
        "Draft spec" => "Draft a strong product spec for this doc.",
        "Improve doc" => {
            "Improve this doc for clarity, completeness, and implementation usefulness."
        }
        "Find gaps" => "Find gaps, risks, missing decisions, and unclear requirements in this doc.",
        "Rewrite section" => {
            "Rewrite the current or most relevant section to be clearer and more actionable."
        }
        "Make implementation plan" => "Create a concrete implementation plan from this doc.",
        other => other,
    }
    .to_string()
}

pub fn quick_action_prompt(relative_doc_path: &Path, action: &str) -> String {
    user_prompt(relative_doc_path, &quick_action_task(action))
}

pub fn start_command(record: &DocAssistantRecord, prompt: &str) -> String {
    match record.provider {
        AgentKind::Claude => format!(
            "claude --permission-mode {} --disallowedTools {} --name {} --model {} --effort {} {}",
            record.access_mode.claude_permission_mode(),
            shell_quote("Bash,NotebookEdit"),
            shell_quote(&record.title()),
            shell_quote(record.model.cli_value().unwrap_or("claude-sonnet-5")),
            shell_quote(record.effort.cli_value()),
            shell_quote(prompt),
        ),
        AgentKind::Codex => {
            let mut parts = vec![
                "codex".to_string(),
                "-s".to_string(),
                record.access_mode.codex_sandbox().to_string(),
                "-a".to_string(),
                record.access_mode.codex_approval_policy().to_string(),
            ];
            if let Some(model) = record.model.cli_value() {
                parts.push("-m".to_string());
                parts.push(shell_quote(model));
            }
            parts.push("-c".to_string());
            parts.push(shell_quote(&format!(
                "model_reasoning_effort=\"{}\"",
                record.effort.cli_value()
            )));
            parts.push(shell_quote(prompt));
            parts.join(" ")
        }
        AgentKind::OpenCode => {
            let mut parts = vec!["opencode".to_string(), "run".to_string()];
            if let Some(model) = record.model_cli_value() {
                parts.push("--model".to_string());
                parts.push(shell_quote(model));
            }
            let effort = record.effort.cli_value();
            if record
                .external_model_variants
                .iter()
                .any(|variant| variant == effort)
            {
                parts.push("--variant".to_string());
                parts.push(shell_quote(effort));
            }
            parts.push(shell_quote(prompt));
            parts.join(" ")
        }
    }
}

pub fn resume_command(kind: AgentKind, access_mode: AgentAccessMode, session_id: &str) -> String {
    match kind {
        AgentKind::Claude => format!(
            "claude --permission-mode {} --disallowedTools {} --resume {}",
            access_mode.claude_permission_mode(),
            shell_quote("Bash,NotebookEdit"),
            shell_quote(session_id)
        ),
        AgentKind::Codex => format!(
            "codex -s {} -a {} resume {}",
            access_mode.codex_sandbox(),
            access_mode.codex_approval_policy(),
            shell_quote(session_id)
        ),
        AgentKind::OpenCode => format!("opencode --session {}", shell_quote(session_id)),
    }
}

pub fn extract_doc_proposal(text: &str) -> Result<Option<String>> {
    let mut proposals = Vec::new();
    let mut remainder = text;
    while let Some(start) = remainder.find(DOC_PROPOSAL_START) {
        let after_start = &remainder[start + DOC_PROPOSAL_START.len()..];
        let Some(end) = after_start.find(DOC_PROPOSAL_END) else {
            anyhow::bail!("proposal is missing {DOC_PROPOSAL_END}");
        };
        proposals.push(clean_proposal_body(&after_start[..end]));
        remainder = &after_start[end + DOC_PROPOSAL_END.len()..];
    }
    match proposals.len() {
        0 => Ok(None),
        1 => Ok(Some(proposals.remove(0))),
        _ => anyhow::bail!("multiple doc proposals found"),
    }
}

fn clean_proposal_body(body: &str) -> String {
    let mut text = body.trim();
    if let Some(stripped) = text.strip_prefix("```markdown") {
        text = stripped.trim_start();
    } else if let Some(stripped) = text.strip_prefix("```md") {
        text = stripped.trim_start();
    } else if let Some(stripped) = text.strip_prefix("```") {
        text = stripped.trim_start();
    }
    if let Some(stripped) = text.strip_suffix("```") {
        text = stripped.trim_end();
    }
    text.trim_matches('\n').to_string()
}

pub fn read_chat_messages(
    kind: AgentKind,
    cwd: &Path,
    session_id: &str,
) -> Vec<DocAssistantMessage> {
    read_chat_transcript_messages(kind, cwd, session_id)
        .into_iter()
        .map(|message| DocAssistantMessage {
            role: message.role,
            text: message.text,
        })
        .collect()
}

pub fn read_chat_transcript_messages(
    kind: AgentKind,
    cwd: &Path,
    session_id: &str,
) -> Vec<DocAssistantTranscriptMessage> {
    let Some(path) = agents::chat_transcript_path(kind, cwd, session_id) else {
        return Vec::new();
    };
    let Ok(file) = fs::File::open(path) else {
        return Vec::new();
    };
    BufReader::new(file)
        .lines()
        .map_while(|line| line.ok())
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(&line).ok())
        .filter_map(|value| transcript_message_from_json(&value))
        .collect()
}

#[cfg(test)]
fn message_from_json(value: &serde_json::Value) -> Option<DocAssistantMessage> {
    transcript_message_from_json(value).map(|message| DocAssistantMessage {
        role: message.role,
        text: message.text,
    })
}

fn transcript_message_from_json(
    value: &serde_json::Value,
) -> Option<DocAssistantTranscriptMessage> {
    if is_tool_transcript_entry(value) {
        return None;
    }
    let role = role_from_value(value)?;
    let text = text_from_value(value)?;
    let text = text.trim().to_string();
    if text.is_empty()
        || is_internal_context_message(&text)
        || is_assistant_tool_preamble(value, &role, &text)
    {
        return None;
    }
    Some(DocAssistantTranscriptMessage {
        role,
        text,
        backend_message_id: backend_message_id_from_value(value),
    })
}

fn is_tool_transcript_entry(value: &serde_json::Value) -> bool {
    json_contains_content_type(value, "tool_result")
        || json_contains_content_type(value, "tool_use")
        || value.pointer("/toolUseResult").is_some()
        || value
            .pointer("/attachment/type")
            .and_then(|value| value.as_str())
            .is_some_and(|kind| {
                matches!(
                    kind,
                    "hook_success" | "hook_error" | "deferred_tools_delta" | "agent_listing_delta"
                )
            })
}

fn json_contains_content_type(value: &serde_json::Value, expected: &str) -> bool {
    match value {
        serde_json::Value::Array(items) => items
            .iter()
            .any(|item| json_contains_content_type(item, expected)),
        serde_json::Value::Object(object) => {
            object
                .get("type")
                .and_then(|value| value.as_str())
                .is_some_and(|kind| kind == expected)
                || object
                    .values()
                    .any(|value| json_contains_content_type(value, expected))
        }
        _ => false,
    }
}

fn is_assistant_tool_preamble(
    value: &serde_json::Value,
    role: &DocAssistantRole,
    text: &str,
) -> bool {
    if !matches!(role, DocAssistantRole::Assistant) {
        return false;
    }
    let stop_reason = value
        .pointer("/message/stop_reason")
        .or_else(|| value.pointer("/stop_reason"))
        .or_else(|| value.pointer("/payload/message/stop_reason"))
        .and_then(|value| value.as_str());
    stop_reason == Some("tool_use") && is_doc_read_preamble(text)
}

fn is_doc_read_preamble(text: &str) -> bool {
    let normalized = text.trim().to_ascii_lowercase();
    let word_count = normalized.split_whitespace().count();
    word_count <= 24
        && (normalized.contains("take a look at the current document")
            || normalized.contains("look at the current document")
            || normalized.contains("read the current document")
            || normalized.contains("get context"))
}

fn is_internal_context_message(text: &str) -> bool {
    let text = text.trim_start();
    text.starts_with("<environment_context>")
        || text.starts_with("<permissions instructions>")
        || text.starts_with("<app-context>")
        || text.starts_with("<collaboration_mode>")
        || text.starts_with("<skills_instructions>")
        || text.starts_with("<plugins_instructions>")
}

fn role_from_value(value: &serde_json::Value) -> Option<DocAssistantRole> {
    let candidates = [
        value.pointer("/message/role"),
        value.pointer("/role"),
        value.pointer("/payload/message/role"),
        value.pointer("/payload/item/role"),
        value.pointer("/payload/role"),
        value.pointer("/type"),
    ];
    candidates
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str())
        .find_map(|role| match role {
            "user" | "human" | "input" | "user_message" => Some(DocAssistantRole::User),
            "assistant" | "agent" | "output" | "assistant_message" | "agent_message" => {
                Some(DocAssistantRole::Assistant)
            }
            _ => None,
        })
}

fn text_from_value(value: &serde_json::Value) -> Option<String> {
    let candidates = [
        value.pointer("/message/content"),
        value.pointer("/content"),
        value.pointer("/payload/message/content"),
        value.pointer("/payload/item/content"),
        value.pointer("/payload/content"),
        value.pointer("/payload/text"),
        value.pointer("/text"),
        value.pointer("/message"),
    ];
    let mut parts = Vec::new();
    for candidate in candidates.into_iter().flatten() {
        collect_text(candidate, &mut parts);
        if !parts.is_empty() {
            break;
        }
    }
    (!parts.is_empty()).then(|| parts.join("\n"))
}

fn backend_message_id_from_value(value: &serde_json::Value) -> Option<String> {
    let candidates = [
        value.pointer("/message/id"),
        value.pointer("/id"),
        value.pointer("/payload/message/id"),
        value.pointer("/payload/item/id"),
        value.pointer("/payload/id"),
    ];
    candidates
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str())
        .find(|id| !id.trim().is_empty())
        .map(str::to_string)
}

fn collect_text(value: &serde_json::Value, parts: &mut Vec<String>) {
    match value {
        serde_json::Value::String(text) => parts.push(text.clone()),
        serde_json::Value::Array(items) => {
            for item in items {
                collect_text(item, parts);
            }
        }
        serde_json::Value::Object(object) => {
            if let Some(text) = object.get("text").and_then(|value| value.as_str()) {
                parts.push(text.to_string());
            } else if let Some(text) = object.get("content").and_then(|value| value.as_str()) {
                parts.push(text.to_string());
            } else if let Some(text) = object.get("output_text").and_then(|value| value.as_str()) {
                parts.push(text.to_string());
            } else if let Some(text) = object.get("input_text").and_then(|value| value.as_str()) {
                parts.push(text.to_string());
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn sample_record(kind: AgentKind) -> DocAssistantRecord {
        let mut record = DocAssistantRecord::new(
            ProjectId(Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap()),
            PathBuf::from("choro_docs/spec.md"),
        );
        record.provider = kind;
        record.model = AgentModel::default_for(kind);
        record
    }

    #[test]
    fn older_store_defaults_records() {
        let store: DocAssistantStoreFile = serde_json::from_str(r#"{"version":1}"#).unwrap();
        assert!(store.assistants.is_empty());
    }

    #[test]
    fn loading_store_migrates_legacy_doc_paths_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("doc_assistants.json");
        let mut legacy = sample_record(AgentKind::Codex);
        legacy.relative_doc_path = PathBuf::from("my_ide_docs/spec.md");
        let store = DocAssistantStoreFile::new(vec![legacy]);
        store.save_to(&path).unwrap();

        let loaded = DocAssistantStoreFile::load_from(&path);

        assert_eq!(
            loaded.assistants[0].relative_doc_path,
            PathBuf::from("choro_docs/spec.md")
        );
        let persisted = fs::read_to_string(path).unwrap();
        assert!(persisted.contains("choro_docs/spec.md"));
        assert!(!persisted.contains("my_ide_docs/spec.md"));
    }

    #[test]
    fn doc_key_uses_repo_relative_path() {
        let project = ProjectId(Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap());
        assert_eq!(
            doc_assistant_key(project, Path::new("choro_docs/features/foo.md")),
            "00000000-0000-0000-0000-000000000001:choro_docs/features/foo.md"
        );
    }

    #[test]
    fn loading_store_migrates_legacy_default_model() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("doc_assistants.json");
        let mut legacy = sample_record(AgentKind::Codex);
        legacy.model = AgentModel::CodexDefault;
        DocAssistantStoreFile::new(vec![legacy])
            .save_to(&path)
            .unwrap();

        let loaded = DocAssistantStoreFile::load_from(&path);

        // Resolves to the current default, so this must not pin a generation.
        let expected = AgentModel::default_for(AgentKind::Codex);
        assert_ne!(expected, AgentModel::CodexDefault);
        assert_eq!(loaded.assistants[0].model, expected);
        let saved =
            serde_json::from_str::<DocAssistantStoreFile>(&fs::read_to_string(path).unwrap())
                .unwrap();
        assert_eq!(saved.assistants[0].model, expected);
    }

    #[test]
    fn start_and_resume_commands_use_provider_session() {
        let mut claude = sample_record(AgentKind::Claude);
        claude.cli_session_id = Some("abc".into());
        assert!(claude
            .start_command("hello")
            .starts_with("claude --permission-mode bypassPermissions --disallowedTools "));
        assert_eq!(
            claude.resume_command(),
            Some(
                "claude --permission-mode bypassPermissions --disallowedTools 'Bash,NotebookEdit' --resume 'abc'"
                    .into()
            )
        );

        let mut codex = sample_record(AgentKind::Codex);
        codex.cli_session_id = Some("def".into());
        assert_eq!(
            codex.start_command("hello"),
            "codex -s danger-full-access -a never -m 'gpt-6-sol' -c 'model_reasoning_effort=\"medium\"' 'hello'"
        );
        assert_eq!(
            codex.resume_command(),
            Some("codex -s danger-full-access -a never resume 'def'".into())
        );

        let mut open_code = sample_record(AgentKind::OpenCode);
        open_code.cli_session_id = Some("ses_123".into());
        open_code.external_model_id = Some("openai/gpt-5.4".into());
        open_code.external_model_label = Some("GPT-5.4".into());
        open_code.external_model_variants = vec!["high".into()];
        open_code.effort = AgentEffort::High;
        assert_eq!(
            open_code.start_command("hello"),
            "opencode run --model 'openai/gpt-5.4' --variant 'high' 'hello'"
        );
        assert_eq!(
            open_code.resume_command(),
            Some("opencode --session 'ses_123'".into())
        );
    }

    #[test]
    fn older_records_default_to_full_access() {
        let mut value = serde_json::to_value(sample_record(AgentKind::Codex)).unwrap();
        value.as_object_mut().unwrap().remove("access_mode");

        let loaded: DocAssistantRecord = serde_json::from_value(value).unwrap();

        assert_eq!(loaded.access_mode, AgentAccessMode::FullAccess);
    }

    #[test]
    fn prompt_requires_repository_grounding_without_embedding_project_content() {
        let prompt = user_prompt(Path::new("choro_docs/spec.md"), "Find gaps");
        assert!(prompt.contains("choro_docs/spec.md"));
        assert!(prompt.contains("Find gaps"));
        assert!(prompt.contains("You are not an implementation agent"));
        assert!(prompt.contains("This document is the only file you may write"));
        assert!(prompt.contains("Be a conversational assistant first"));
        assert!(prompt.contains("\"let's talk\""));
        assert!(prompt.contains("\"I'm not sure\""));
        assert!(prompt.contains("Do not edit the document just because the topic is in the doc"));
        assert!(prompt.contains("Document naming on the first edit"));
        assert!(prompt.contains("untitled-N.choro"));
        assert!(prompt.contains("Do not rename or move the file yourself"));
        assert!(prompt.contains("the user has already named the document"));
        assert!(prompt.contains("Project grounding is mandatory"));
        assert!(prompt.contains("Before your first substantive response"));
        assert!(prompt.contains("always inspect the relevant implementation"));
        assert!(prompt.contains("Briefly name the concrete project files"));
        assert!(prompt.contains("You may read the entire project"));
        assert!(!prompt.contains("# Product Spec"));
    }

    #[test]
    fn proposal_extraction_accepts_one_fenced_doc() {
        let text = "ok\nDOC_PROPOSAL\n```markdown\n# Title\n\nBody\n```\nEND_DOC_PROPOSAL";
        assert_eq!(
            extract_doc_proposal(text).unwrap(),
            Some("# Title\n\nBody".into())
        );
    }

    #[test]
    fn proposal_extraction_rejects_bad_shapes() {
        assert!(extract_doc_proposal("DOC_PROPOSAL\n# A").is_err());
        assert!(extract_doc_proposal(
            "DOC_PROPOSAL\n# A\nEND_DOC_PROPOSAL\nDOC_PROPOSAL\n# B\nEND_DOC_PROPOSAL"
        )
        .is_err());
    }

    #[test]
    fn transcript_parser_handles_claude_and_codex_shapes() {
        let claude: serde_json::Value = serde_json::json!({
            "type": "assistant",
            "message": {"role": "assistant", "content": [{"type": "text", "text": "Hello"}]}
        });
        let codex: serde_json::Value = serde_json::json!({
            "type": "response_item",
            "payload": {
                "item": {
                    "type": "message",
                    "role": "user",
                    "content": [{"type": "input_text", "text": "Question"}]
                }
            }
        });
        assert_eq!(
            message_from_json(&claude),
            Some(DocAssistantMessage {
                role: DocAssistantRole::Assistant,
                text: "Hello".into()
            })
        );
        assert_eq!(
            message_from_json(&codex),
            Some(DocAssistantMessage {
                role: DocAssistantRole::User,
                text: "Question".into()
            })
        );

        let codex_assistant: serde_json::Value = serde_json::json!({
            "type": "response_item",
            "payload": {
                "type": "message",
                "id": "msg_123",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "Full answer"}]
            }
        });
        assert_eq!(
            transcript_message_from_json(&codex_assistant),
            Some(DocAssistantTranscriptMessage {
                role: DocAssistantRole::Assistant,
                text: "Full answer".into(),
                backend_message_id: Some("msg_123".into())
            })
        );
    }

    #[test]
    fn transcript_parser_hides_doc_read_tool_noise() {
        let preamble: serde_json::Value = serde_json::json!({
            "type": "assistant",
            "message": {
                "role": "assistant",
                "content": [{"type": "text", "text": "Let me take a look at the current document to get context."}],
                "stop_reason": "tool_use"
            }
        });
        let read_tool: serde_json::Value = serde_json::json!({
            "type": "assistant",
            "message": {
                "role": "assistant",
                "content": [{
                    "type": "tool_use",
                    "name": "Read",
                    "input": {"file_path": "/tmp/doc.md"}
                }]
            }
        });
        let read_result: serde_json::Value = serde_json::json!({
            "type": "user",
            "message": {
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "content": "1 # Product Spec\n2\n3 ## Goal"
                }]
            }
        });

        assert_eq!(message_from_json(&preamble), None);
        assert_eq!(message_from_json(&read_tool), None);
        assert_eq!(message_from_json(&read_result), None);
    }
}

/// Keep historical retired design conversations out of ordinary document lists.
pub fn is_retired_design_record(path: &std::path::Path) -> bool {
    path.starts_with(std::path::Path::new(".choro/assistants/penpot"))
}
