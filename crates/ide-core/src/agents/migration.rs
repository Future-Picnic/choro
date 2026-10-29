//! Explicit, uncapped discovery and preparation of local provider conversations.

use std::collections::HashSet;
use std::io::{BufRead, BufReader};

use super::*;
use crate::doc_assistant::{read_migration_transcript_file, DocAssistantRole};
use crate::local_store::StoredChatMessage;

#[derive(Clone, Debug)]
pub struct Conversation {
    pub chat: AgentChat,
    pub transcript_path: PathBuf,
}

#[derive(Clone)]
pub struct PreparedConversation {
    pub agent: AgentRecord,
    pub messages: Vec<StoredChatMessage>,
}

impl Conversation {
    pub fn already_imported(&self, project: ProjectId, records: &[AgentRecord]) -> bool {
        records.iter().any(|agent| {
            agent.project_id == project
                && agent.provider == self.chat.kind
                && (agent.cli_session_id.as_deref() == Some(self.chat.session_id.as_str())
                    || agent.chat_session_id.as_deref() == Some(self.chat.session_id.as_str()))
        })
    }

    pub fn prepare(&self, project: ProjectId, path: &Path) -> Result<PreparedConversation> {
        let mut transcript = read_migration_transcript_file(&self.transcript_path)?;
        // Codex may record the same message as both a response item and an
        // event. Keep one visible copy, retaining the backend identity.
        transcript.dedup_by(|next, previous| {
            if next.role != previous.role || next.text != previous.text {
                return false;
            }
            if previous.backend_message_id.is_none() {
                previous.backend_message_id = next.backend_message_id.clone();
            }
            true
        });
        anyhow::ensure!(
            !transcript.is_empty(),
            "{} has no readable conversation messages",
            self.chat.title
        );
        let model = AgentModel::default_for(self.chat.kind);
        let mut agent = AgentRecord::new(
            project,
            path.to_path_buf(),
            self.chat.title.clone(),
            "",
            self.chat.kind,
            model,
            model.default_effort(),
            AgentAccessMode::AskForApproval,
        );
        agent.status = AgentStatus::Todo;
        agent.runtime = AgentRuntimeKind::Chat;
        agent.updated_at = self
            .chat
            .updated_at
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        agent.created_at = agent.updated_at;
        agent.started_at = Some(agent.created_at);
        agent.cli_session_id = Some(self.chat.session_id.clone());
        agent.chat_session_id = Some(self.chat.session_id.clone());
        let messages = transcript
            .into_iter()
            .enumerate()
            .map(|(index, message)| StoredChatMessage {
                id: Uuid::new_v4(),
                agent_id: agent.id,
                role: match message.role {
                    DocAssistantRole::User => "user",
                    DocAssistantRole::Assistant => "assistant",
                }
                .into(),
                text: message.text,
                sequence: index as i64 + 1,
                created_at: agent.created_at,
                backend_message_id: message.backend_message_id,
            })
            .collect();
        Ok(PreparedConversation { agent, messages })
    }
}

pub fn discover(cwd: &Path) -> Result<Vec<Conversation>> {
    let home = dirs::home_dir().context("Could not locate your home folder")?;
    let claude = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".claude"));
    let codex = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".codex"));
    discover_in(cwd, &claude, &codex)
}

fn entries(path: &Path) -> Result<Vec<fs::DirEntry>> {
    match fs::read_dir(path) {
        Ok(entries) => entries
            .collect::<std::io::Result<Vec<_>>>()
            .with_context(|| format!("Could not scan {}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error).with_context(|| format!("Could not scan {}", path.display())),
    }
}

fn discover_in(cwd: &Path, claude: &Path, codex: &Path) -> Result<Vec<Conversation>> {
    let canonical = fs::canonicalize(cwd)
        .with_context(|| format!("Could not open project {}", cwd.display()))?;
    let mut conversations = Vec::new();
    let mut seen = HashSet::new();
    // Include the original spelling and canonical path for symlinked projects.
    for project_path in [cwd, canonical.as_path()] {
        let slug: String = project_path
            .to_string_lossy()
            .chars()
            .map(|c| match c {
                '/' | '.' | '_' | ' ' => '-',
                other => other,
            })
            .collect();
        for entry in entries(&claude.join("projects").join(slug))? {
            let path = entry.path();
            if !entry.file_type()?.is_file()
                || path.extension().and_then(|ext| ext.to_str()) != Some("jsonl")
            {
                continue;
            }
            // Claude's folder encoding is lossy (foo.bar and foo_bar collide).
            // Prefer transcript metadata over the bucket's spelling.
            let mut belongs = false;
            for line in BufReader::new(fs::File::open(&path)?).lines().take(64) {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line?) {
                    if let Some(recorded) = value["cwd"].as_str() {
                        let recorded = Path::new(recorded);
                        belongs = recorded == cwd
                            || fs::canonicalize(recorded).ok().as_ref() == Some(&canonical);
                        break;
                    }
                }
            }
            if !belongs {
                continue;
            }
            let Some(session_id) = path
                .file_stem()
                .and_then(|id| id.to_str())
                .map(str::to_owned)
            else {
                continue;
            };
            if session_id.starts_with("agent-")
                || !seen.insert((AgentKind::Claude, session_id.clone()))
            {
                continue;
            }
            conversations.push(Conversation {
                chat: AgentChat {
                    kind: AgentKind::Claude,
                    title: claude_chat_title(&path)
                        .unwrap_or_else(|| format!("Claude conversation {session_id}")),
                    session_id,
                    updated_at: entry.metadata()?.modified()?,
                },
                transcript_path: path,
            });
        }
    }
    let mut titles = HashMap::new();
    let index = codex.join("session_index.jsonl");
    if index.exists() {
        for line in BufReader::new(fs::File::open(&index)?).lines() {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line?) {
                if let (Some(id), Some(title)) =
                    (value["id"].as_str(), value["thread_name"].as_str())
                {
                    titles.insert(id.to_string(), title.to_string());
                }
            }
        }
    }
    let mut directories = vec![codex.join("sessions"), codex.join("archived_sessions")];
    while let Some(directory) = directories.pop() {
        for entry in entries(&directory)? {
            let path = entry.path();
            let kind = entry.file_type()?;
            if kind.is_dir() {
                directories.push(path);
                continue;
            }
            if !kind.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }
            let Some((session_id, session_cwd)) = codex_session_cwd(&path) else {
                continue;
            };
            if session_cwd != cwd
                && fs::canonicalize(&session_cwd).ok().as_ref() != Some(&canonical)
            {
                continue;
            }
            if !seen.insert((AgentKind::Codex, session_id.clone())) {
                continue;
            }
            remember_codex_rollout_path(&session_id, &path);
            conversations.push(Conversation {
                chat: AgentChat {
                    kind: AgentKind::Codex,
                    title: titles
                        .get(&session_id)
                        .cloned()
                        .unwrap_or_else(|| format!("Codex conversation {session_id}")),
                    session_id,
                    updated_at: entry.metadata()?.modified()?,
                },
                transcript_path: path,
            });
        }
    }
    conversations.sort_by(|left, right| {
        right
            .chat
            .updated_at
            .cmp(&left.chat.updated_at)
            .then_with(|| left.chat.session_id.cmp(&right.chat.session_id))
    });
    Ok(conversations)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_is_project_scoped_and_uncapped_and_prepares_resumable_history() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        fs::create_dir(&project).unwrap();
        let codex = root.path().join("codex");
        let sessions = codex.join("sessions/2026/10/29");
        fs::create_dir_all(&sessions).unwrap();
        for index in 0..510 {
            let metadata = serde_json::json!({"type":"session_meta","payload":{"id":format!("session-{index}"),"cwd":project}});
            fs::write(sessions.join(format!("rollout-{index}.jsonl")), format!("{metadata}\n{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"user_message\",\"message\":\"Hello\"}}}}\n")).unwrap();
        }
        let other =
            serde_json::json!({"type":"session_meta","payload":{"id":"other","cwd":root.path()}});
        fs::write(sessions.join("other.jsonl"), other.to_string()).unwrap();
        let chats = discover_in(&project, &root.path().join("claude"), &codex).unwrap();
        assert_eq!(chats.len(), 510);
        let project_id = ProjectId::new();
        let prepared = chats[0].prepare(project_id, &project).unwrap();
        assert_eq!(prepared.messages[0].text, "Hello");
        assert_eq!(prepared.agent.runtime, AgentRuntimeKind::Chat);
        assert_eq!(
            prepared.agent.cli_session_id.as_deref(),
            Some(chats[0].chat.session_id.as_str())
        );
        assert!(chats[0].already_imported(project_id, &[prepared.agent.clone()]));
        assert!(!chats[0].already_imported(ProjectId::new(), &[prepared.agent]));
    }

    #[test]
    fn claude_discovery_and_duplicate_identity_include_provider() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path();
        let claude = root.path().join("claude");
        let slug: String = project
            .to_string_lossy()
            .chars()
            .map(|c| match c {
                '/' | '.' | '_' | ' ' => '-',
                other => other,
            })
            .collect();
        let sessions = claude.join("projects").join(slug);
        fs::create_dir_all(&sessions).unwrap();
        for index in 0..60 {
            let user = serde_json::json!({"type":"user","cwd":project,"message":{"role":"user","content":"Existing work"}});
            let assistant = serde_json::json!({"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"The answer"},{"type":"tool_use","id":"tool-1","name":"Read","input":{"path":"file.rs"}}]}});
            fs::write(
                sessions.join(format!("session-{index}.jsonl")),
                format!("{user}\n{assistant}\n"),
            )
            .unwrap();
        }
        let chats = discover_in(project, &claude, &root.path().join("codex")).unwrap();
        assert_eq!(chats.len(), 60);
        let id = ProjectId::new();
        let mut prepared = chats[0].prepare(id, project).unwrap();
        assert_eq!(prepared.messages.len(), 2);
        assert_eq!(prepared.messages[1].role, "assistant");
        assert_eq!(prepared.messages[1].text, "The answer");
        prepared.agent.provider = AgentKind::Codex;
        assert!(!chats[0].already_imported(id, &[prepared.agent]));
        fs::write(&chats[0].transcript_path, "{}").unwrap();
        assert!(chats[0].prepare(id, project).is_err());
    }

    #[test]
    fn claude_slug_collisions_do_not_mix_project_histories() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("app.one");
        let other = root.path().join("app_one");
        fs::create_dir(&project).unwrap();
        fs::create_dir(&other).unwrap();
        let claude = root.path().join("claude");
        let slug: String = project
            .to_string_lossy()
            .chars()
            .map(|c| match c {
                '/' | '.' | '_' | ' ' => '-',
                other => other,
            })
            .collect();
        let bucket = claude.join("projects").join(slug);
        fs::create_dir_all(&bucket).unwrap();
        for (id, cwd) in [("selected", &project), ("other", &other)] {
            let entry = serde_json::json!({"type":"user","cwd":cwd,"message":{"role":"user","content":"Hello"}});
            fs::write(bucket.join(format!("{id}.jsonl")), entry.to_string()).unwrap();
        }
        let found = discover_in(&project, &claude, &root.path().join("codex")).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].chat.session_id, "selected");
    }
}
