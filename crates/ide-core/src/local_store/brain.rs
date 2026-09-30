use super::*;
use std::time::Duration;

pub const MAX_AGENT_SUMMARY_CHARS: usize = 12_000;
pub const MAX_AGENT_OUTCOME_CHARS: usize = 220;
pub const MAX_AGENT_MESSAGE_CHARS: usize = 8_000;
const AGENT_REPLY_LOCK_ATTEMPTS: usize = 4;

fn is_database_lock_error(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(|cause| cause.to_string().to_ascii_lowercase().contains("lock"))
}

fn retry_database_lock_with_delay<T>(
    mut operation: impl FnMut() -> Result<T>,
    retry_delay: Duration,
) -> Result<T> {
    for attempt in 0..AGENT_REPLY_LOCK_ATTEMPTS {
        match operation() {
            Ok(value) => return Ok(value),
            Err(error)
                if is_database_lock_error(&error) && attempt + 1 < AGENT_REPLY_LOCK_ATTEMPTS =>
            {
                std::thread::sleep(retry_delay.saturating_mul((attempt + 1) as u32));
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!("the bounded database retry loop always returns")
}

fn retry_database_lock<T>(operation: impl FnMut() -> Result<T>) -> Result<T> {
    retry_database_lock_with_delay(operation, Duration::from_millis(100))
}

async fn load_agent_summary_async(
    conn: &Connection,
    agent_id: Uuid,
) -> Result<Option<StoredAgentSummary>> {
    let mut rows = conn
        .query(
            "SELECT agent_id, summary_text, outcome_text, last_summarized_sequence,
                    updated_at, edited_by_user
             FROM agent_summaries WHERE agent_id = ?1",
            [agent_id.to_string()],
        )
        .await?;
    let Some(row) = rows.next().await? else {
        return Ok(None);
    };
    Ok(Some(StoredAgentSummary {
        agent_id: parse_uuid(&row.get::<String>(0)?)?,
        summary_text: row.get(1)?,
        outcome_text: opt_text(&row, 2)?,
        last_summarized_sequence: row.get(3)?,
        updated_at: i64_to_u64(row.get(4)?)?,
        edited_by_user: row.get::<i64>(5)? != 0,
    }))
}

pub(super) async fn load_all_agent_summaries_async(
    conn: &Connection,
) -> Result<Vec<StoredAgentSummary>> {
    let mut rows = conn
        .query(
            "SELECT agent_id, summary_text, outcome_text, last_summarized_sequence,
                    updated_at, edited_by_user
             FROM agent_summaries ORDER BY updated_at DESC",
            (),
        )
        .await?;
    let mut summaries = Vec::new();
    while let Some(row) = rows.next().await? {
        summaries.push(StoredAgentSummary {
            agent_id: parse_uuid(&row.get::<String>(0)?)?,
            summary_text: row.get(1)?,
            outcome_text: opt_text(&row, 2)?,
            last_summarized_sequence: row.get(3)?,
            updated_at: i64_to_u64(row.get(4)?)?,
            edited_by_user: row.get::<i64>(5)? != 0,
        });
    }
    Ok(summaries)
}

pub(super) async fn insert_stored_agent_summary_async(
    conn: &Connection,
    summary: &StoredAgentSummary,
) -> Result<()> {
    conn.execute(
        "INSERT INTO agent_summaries
         (agent_id, summary_text, outcome_text, last_summarized_sequence, updated_at, edited_by_user)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(agent_id) DO UPDATE SET
             summary_text = excluded.summary_text,
             outcome_text = excluded.outcome_text,
             last_summarized_sequence = excluded.last_summarized_sequence,
             updated_at = excluded.updated_at,
             edited_by_user = excluded.edited_by_user",
        params![
            summary.agent_id.to_string(),
            summary.summary_text.as_str(),
            summary.outcome_text.clone(),
            summary.last_summarized_sequence,
            u64_to_i64(summary.updated_at)?,
            bool_to_i64(summary.edited_by_user),
        ],
    )
    .await?;
    Ok(())
}

pub(super) async fn insert_stored_agent_message_async(
    conn: &Connection,
    message: &StoredAgentMessage,
) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO agent_messages
         (id, source_agent_id, target_agent_id, text, kind, event_key, created_at, delivered_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            message.id.to_string(),
            message.source_agent_id.to_string(),
            message.target_agent_id.to_string(),
            message.text.as_str(),
            message.kind.as_str(),
            message.event_key.clone(),
            u64_to_i64(message.created_at)?,
            message.delivered_at.map(u64_to_i64).transpose()?,
        ],
    )
    .await?;
    Ok(())
}

pub(super) async fn load_all_agent_messages_async(
    conn: &Connection,
) -> Result<Vec<StoredAgentMessage>> {
    let mut rows = conn
        .query(
            "SELECT agent_messages.id, agent_messages.source_agent_id,
                    agent_messages.target_agent_id, agents.title,
                    agent_messages.text, agent_messages.kind,
                    agent_messages.event_key, agent_messages.created_at,
                    agent_messages.delivered_at
             FROM agent_messages
             INNER JOIN agents ON agents.id = agent_messages.source_agent_id
             ORDER BY agent_messages.created_at ASC",
            (),
        )
        .await?;
    let mut messages = Vec::new();
    while let Some(row) = rows.next().await? {
        messages.push(StoredAgentMessage {
            id: parse_uuid(&row.get::<String>(0)?)?,
            source_agent_id: parse_uuid(&row.get::<String>(1)?)?,
            target_agent_id: parse_uuid(&row.get::<String>(2)?)?,
            source_title: row.get(3)?,
            text: row.get(4)?,
            kind: row.get(5)?,
            event_key: opt_text(&row, 6)?,
            created_at: i64_to_u64(row.get(7)?)?,
            delivered_at: opt_i64(&row, 8)?.map(i64_to_u64).transpose()?,
        });
    }
    Ok(messages)
}

fn normalized_summary_text(text: &str) -> Result<String> {
    let text = text.trim().to_string();
    anyhow::ensure!(!text.is_empty(), "summary text is empty");
    anyhow::ensure!(
        text.chars().count() <= MAX_AGENT_SUMMARY_CHARS,
        "summary text must be at most {MAX_AGENT_SUMMARY_CHARS} characters"
    );
    Ok(text)
}

fn normalized_outcome_text(text: Option<&str>) -> Result<Option<String>> {
    let Some(text) = text else {
        return Ok(None);
    };
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    anyhow::ensure!(!text.is_empty(), "summary outcome is empty");
    anyhow::ensure!(
        text.chars().count() <= MAX_AGENT_OUTCOME_CHARS,
        "summary outcome must be at most {MAX_AGENT_OUTCOME_CHARS} characters"
    );
    Ok(Some(text))
}

fn search_terms(input: &str) -> Result<Vec<String>> {
    let terms = input
        .split(|character: char| !character.is_alphanumeric() && character != '_')
        .filter(|term| !term.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>();
    anyhow::ensure!(!terms.is_empty(), "provide a search query");
    Ok(terms)
}

fn search_snippet(text: &str, query: &str) -> String {
    let terms = query
        .split(|character: char| !character.is_alphanumeric() && character != '_')
        .filter(|term| !term.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>();
    let words = text.split_whitespace().collect::<Vec<_>>();
    if words.is_empty() {
        return String::new();
    }
    let matching_index = words.iter().position(|word| {
        let word = word.to_lowercase();
        terms.iter().any(|term| word.contains(term))
    });
    let start = matching_index.unwrap_or_default().saturating_sub(6);
    let end = (start + 18).min(words.len());
    format!(
        "{}{}{}",
        if start > 0 { "… " } else { "" },
        words[start..end].join(" "),
        if end < words.len() { " …" } else { "" }
    )
}

async fn search_agent_documents_async(
    conn: &Connection,
    project: ProjectId,
    search_terms: &[String],
    display_query: &str,
    file_scope: &[String],
    limit: usize,
) -> Result<Vec<StoredAgentSearchResult>> {
    // Search the source tables directly. Turso's indexed FTS writes have
    // crashed in production; this bounded background scan keeps Brain search
    // available without coupling message durability to that index.
    let fetch_limit = limit.saturating_mul(100).clamp(100, 5_000);
    let mut rows = conn
        .query(
            "SELECT agents.id, agents.project_id, agents.title, agents.status,
                    COALESCE(agent_summaries.summary_text, ''),
                    COALESCE(agent_summaries.outcome_text, ''),
                    COALESCE(agent_summaries.updated_at, agents.updated_at)
             FROM agents
             LEFT JOIN agent_summaries ON agent_summaries.agent_id = agents.id
             WHERE agents.project_id = ?1
             ORDER BY 7 DESC LIMIT ?2",
            params![project.0.to_string(), i64::try_from(fetch_limit)?],
        )
        .await?;
    let mut results = Vec::new();
    while results.len() < limit {
        let Some(row) = rows.next().await? else { break };
        let agent_id = parse_uuid(&row.get::<String>(0)?)?;
        if !agent_matches_file_scope_async(conn, agent_id, file_scope).await? {
            continue;
        }
        let title: String = row.get(2)?;
        let summary_text: String = row.get(4)?;
        let outcome_text: String = row.get(5)?;
        let searchable = format!("{title}\n{summary_text}\n{outcome_text}").to_lowercase();
        if !search_terms.iter().all(|term| searchable.contains(term)) {
            continue;
        }
        let snippet_source = if summary_text.is_empty() {
            title.as_str()
        } else {
            summary_text.as_str()
        };
        let snippet = search_snippet(snippet_source, display_query);
        results.push(StoredAgentSearchResult {
            agent_id,
            title,
            status: row.get(3)?,
            snippet,
            summary_text: (!summary_text.is_empty()).then_some(summary_text),
            updated_at: i64_to_u64(row.get(6)?)?,
        });
    }
    Ok(results)
}

fn normalized_file_scope(files: &[String]) -> Vec<String> {
    let mut files = files
        .iter()
        .map(|path| path.trim().trim_start_matches("./").replace('\\', "/"))
        .filter(|path| !path.is_empty())
        .collect::<Vec<_>>();
    files.sort();
    files.dedup();
    files
}

async fn load_agent_file_scope_async(conn: &Connection, agent_id: Uuid) -> Result<Vec<String>> {
    let mut rows = conn
        .query(
            "SELECT path FROM agent_changed_files WHERE agent_id = ?1 ORDER BY path ASC",
            [agent_id.to_string()],
        )
        .await?;
    let mut files = Vec::new();
    while let Some(row) = rows.next().await? {
        files.push(row.get::<String>(0)?);
    }
    Ok(normalized_file_scope(&files))
}

async fn agent_matches_file_scope_async(
    conn: &Connection,
    agent_id: Uuid,
    file_scope: &[String],
) -> Result<bool> {
    if file_scope.is_empty() {
        return Ok(true);
    }
    let wanted = file_scope
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let mut rows = conn
        .query(
            "SELECT path FROM agent_changed_files WHERE agent_id = ?1",
            [agent_id.to_string()],
        )
        .await?;
    while let Some(row) = rows.next().await? {
        let path = row
            .get::<String>(0)?
            .trim()
            .trim_start_matches("./")
            .replace('\\', "/");
        if wanted.contains(path.as_str()) {
            return Ok(true);
        }
    }
    Ok(false)
}

async fn load_agent_search_document_async(
    conn: &Connection,
    project: ProjectId,
    agent_id: Uuid,
    snippet: String,
) -> Result<Option<StoredAgentSearchResult>> {
    let mut rows = conn
        .query(
            "SELECT agents.id, agents.title, agents.status,
                    COALESCE(agent_summaries.summary_text, ''),
                    COALESCE(agent_summaries.updated_at, agents.updated_at)
             FROM agents
             LEFT JOIN agent_summaries ON agent_summaries.agent_id = agents.id
             WHERE agents.id = ?1 AND agents.project_id = ?2",
            (agent_id.to_string(), project.0.to_string()),
        )
        .await?;
    let Some(row) = rows.next().await? else {
        return Ok(None);
    };
    let summary_text: String = row.get(3)?;
    Ok(Some(StoredAgentSearchResult {
        agent_id: parse_uuid(&row.get::<String>(0)?)?,
        title: row.get(1)?,
        status: row.get(2)?,
        snippet,
        summary_text: (!summary_text.is_empty()).then_some(summary_text),
        updated_at: i64_to_u64(row.get(4)?)?,
    }))
}

async fn brain_scope_async(conn: &Connection, requester: Uuid) -> Result<ProjectId> {
    let mut rows = conn
        .query(
            "SELECT project_id FROM agents WHERE id = ?1",
            [requester.to_string()],
        )
        .await?;
    let Some(row) = rows.next().await? else {
        return Err(anyhow!("the requesting agent no longer exists"));
    };
    Ok(ProjectId(parse_uuid(&row.get::<String>(0)?)?))
}

impl LocalStore {
    /// App-side start context for the new agent. MCP callers use
    /// `search_agents`, which applies the same project boundary.
    pub fn search_project_agents(
        &self,
        project: ProjectId,
        query: &str,
        limit: usize,
    ) -> Result<Vec<StoredAgentSearchResult>> {
        self.search_project_agents_for_files(project, query, &[], limit)
    }

    /// App-side related-work search, optionally narrowed to files explicitly
    /// attached to the new agent's first turn.
    pub fn search_project_agents_for_files(
        &self,
        project: ProjectId,
        query: &str,
        files: &[String],
        limit: usize,
    ) -> Result<Vec<StoredAgentSearchResult>> {
        let display_query = query.trim().to_string();
        let query = search_terms(&display_query)?;
        let file_scope = normalized_file_scope(files);
        let limit = limit.clamp(1, 10);
        self.rt.block_on(async {
            let conn = self.connect().await?;
            search_agent_documents_async(&conn, project, &query, &display_query, &file_scope, limit)
                .await
        })
    }

    /// Replace one agent's living summary. Agent-authored saves cover every
    /// chat row committed before the tool call; user edits retain that cursor.
    pub fn save_agent_summary(
        &self,
        agent_id: Uuid,
        text: &str,
        outcome: Option<&str>,
        edited_by_user: bool,
    ) -> Result<StoredAgentSummary> {
        let summary_text = normalized_summary_text(text)?;
        let outcome_text = normalized_outcome_text(outcome)?;
        self.rt.block_on(async {
            let conn = self.connect().await?;
            let mut agent_rows = conn
                .query("SELECT 1 FROM agents WHERE id = ?1", [agent_id.to_string()])
                .await?;
            anyhow::ensure!(agent_rows.next().await?.is_some(), "agent no longer exists");
            drop(agent_rows);

            let last_summarized_sequence = if edited_by_user {
                load_agent_summary_async(&conn, agent_id)
                    .await?
                    .map(|summary| summary.last_summarized_sequence)
                    .unwrap_or(-1)
            } else {
                let mut rows = conn
                    .query(
                        "SELECT COALESCE(MAX(sequence), -1) FROM chat_messages WHERE agent_id = ?1",
                        [agent_id.to_string()],
                    )
                    .await?;
                rows.next()
                    .await?
                    .map(|row| row.get(0))
                    .transpose()?
                    .unwrap_or(-1)
            };
            let summary = StoredAgentSummary {
                agent_id,
                summary_text,
                outcome_text,
                last_summarized_sequence,
                updated_at: unix_now(),
                edited_by_user,
            };
            insert_stored_agent_summary_async(&conn, &summary).await?;
            Ok(summary)
        })
    }

    pub fn load_agent_summary(&self, agent_id: Uuid) -> Result<Option<StoredAgentSummary>> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            load_agent_summary_async(&conn, agent_id).await
        })
    }

    pub fn load_all_agent_summaries(&self) -> Result<Vec<StoredAgentSummary>> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            load_all_agent_summaries_async(&conn).await
        })
    }

    pub fn agent_summary_refresh_due(
        &self,
        agent_id: Uuid,
        minimum_new_messages: i64,
        minimum_interval_secs: u64,
    ) -> Result<bool> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            let summary = load_agent_summary_async(&conn, agent_id).await?;
            let mut rows = conn
                .query(
                    "SELECT COALESCE(MAX(sequence), -1) FROM chat_messages WHERE agent_id = ?1",
                    [agent_id.to_string()],
                )
                .await?;
            let latest: i64 = rows
                .next()
                .await?
                .map(|row| row.get(0))
                .transpose()?
                .unwrap_or(-1);
            let (covered, updated_at) = summary
                .map(|summary| (summary.last_summarized_sequence, summary.updated_at))
                .unwrap_or((-1, 0));
            Ok(latest.saturating_sub(covered) >= minimum_new_messages
                && unix_now().saturating_sub(updated_at) >= minimum_interval_secs)
        })
    }

    pub fn search_agents(
        &self,
        requester: Uuid,
        query: &str,
        include_messages: bool,
        limit: usize,
    ) -> Result<Vec<StoredAgentSearchResult>> {
        self.search_agents_for_files(requester, query, include_messages, Some(&[]), limit)
    }

    /// Search within the requester's project and, unless the caller supplies
    /// an explicit file list, within files the requester has already touched.
    /// Passing `Some(&[])` deliberately requests project-wide results.
    pub fn search_agents_for_files(
        &self,
        requester: Uuid,
        query: &str,
        include_messages: bool,
        files: Option<&[String]>,
        limit: usize,
    ) -> Result<Vec<StoredAgentSearchResult>> {
        let display_query = query.trim().to_string();
        let query = search_terms(&display_query)?;
        let limit = limit.clamp(1, 50);
        self.rt.block_on(async {
            let conn = self.connect().await?;
            let project = brain_scope_async(&conn, requester).await?;
            let file_scope = match files {
                Some(files) => normalized_file_scope(files),
                None => load_agent_file_scope_async(&conn, requester).await?,
            };
            let mut results = search_agent_documents_async(
                &conn,
                project,
                &query,
                &display_query,
                &file_scope,
                limit,
            )
            .await?;
            let mut seen = results
                .iter()
                .map(|result| result.agent_id)
                .collect::<HashSet<_>>();

            if include_messages && results.len() < limit {
                let fetch_limit = limit.saturating_mul(100).clamp(100, 5_000);
                let mut rows = conn
                    .query(
                        "SELECT id, agent_id, text FROM chat_messages
                         ORDER BY created_at DESC LIMIT ?1",
                        [i64::try_from(fetch_limit)?],
                    )
                    .await?;
                while results.len() < limit {
                    let Some(row) = rows.next().await? else { break };
                    let agent_id = parse_uuid(&row.get::<String>(1)?)?;
                    if !seen.insert(agent_id) {
                        continue;
                    }
                    if !agent_matches_file_scope_async(&conn, agent_id, &file_scope).await? {
                        continue;
                    }
                    let text: String = row.get(2)?;
                    let searchable = text.to_lowercase();
                    if !query.iter().all(|term| searchable.contains(term)) {
                        continue;
                    }
                    if let Some(result) = load_agent_search_document_async(
                        &conn,
                        project,
                        agent_id,
                        search_snippet(&text, &display_query),
                    )
                    .await?
                    {
                        results.push(result);
                    }
                }
            }
            Ok(results)
        })
    }

    pub fn recall_agent(
        &self,
        requester: Uuid,
        target: Uuid,
        before_sequence: Option<i64>,
        limit: usize,
    ) -> Result<StoredAgentRecallPage> {
        let limit = limit.clamp(1, 100);
        self.rt.block_on(async {
            let conn = self.connect().await?;
            let project = brain_scope_async(&conn, requester).await?;
            let mut rows = conn
                .query(
                    "SELECT title FROM agents WHERE id = ?1 AND project_id = ?2",
                    (target.to_string(), project.0.to_string()),
                )
                .await?;
            let Some(row) = rows.next().await? else {
                return Err(anyhow!("target agent is not in this project"));
            };
            let title: String = row.get(0)?;
            drop(rows);
            let query_limit = i64::try_from(limit.saturating_add(1))?;
            let mut rows = match before_sequence {
                Some(before) => {
                    conn.query(
                        "SELECT id, agent_id, role, text, sequence, created_at, backend_message_id
                         FROM chat_messages WHERE agent_id = ?1 AND sequence < ?2
                         ORDER BY sequence DESC LIMIT ?3",
                        params![target.to_string(), before, query_limit],
                    )
                    .await?
                }
                None => {
                    conn.query(
                        "SELECT id, agent_id, role, text, sequence, created_at, backend_message_id
                         FROM chat_messages WHERE agent_id = ?1
                         ORDER BY sequence DESC LIMIT ?2",
                        params![target.to_string(), query_limit],
                    )
                    .await?
                }
            };
            let mut messages = Vec::new();
            while let Some(row) = rows.next().await? {
                messages.push(StoredChatMessage {
                    id: parse_uuid(&row.get::<String>(0)?)?,
                    agent_id: parse_uuid(&row.get::<String>(1)?)?,
                    role: row.get(2)?,
                    text: row.get(3)?,
                    sequence: row.get(4)?,
                    created_at: i64_to_u64(row.get(5)?)?,
                    backend_message_id: opt_text(&row, 6)?,
                });
            }
            let has_more = messages.len() > limit;
            if has_more {
                messages.truncate(limit);
            }
            messages.reverse();
            Ok(StoredAgentRecallPage {
                agent_id: target,
                title,
                summary: load_agent_summary_async(&conn, target).await?,
                next_before_sequence: messages.first().map(|message| message.sequence),
                messages,
                has_more,
            })
        })
    }

    pub fn send_agent_message(
        &self,
        source: Uuid,
        target: Uuid,
        text: &str,
        kind: &str,
        event_key: Option<String>,
    ) -> Result<StoredAgentMessage> {
        let text = text.trim().to_string();
        anyhow::ensure!(!text.is_empty(), "message text is empty");
        anyhow::ensure!(
            text.chars().count() <= MAX_AGENT_MESSAGE_CHARS,
            "message text must be at most {MAX_AGENT_MESSAGE_CHARS} characters"
        );
        anyhow::ensure!(source != target, "choose another agent");
        self.rt.block_on(async {
            let conn = self.connect().await?;
            let project = brain_scope_async(&conn, source).await?;
            let mut rows = conn
                .query(
                    "SELECT title FROM agents WHERE id = ?1 AND project_id = ?2",
                    (source.to_string(), project.0.to_string()),
                )
                .await?;
            let Some(source_row) = rows.next().await? else {
                return Err(anyhow!("source agent is not in this project"));
            };
            let source_title: String = source_row.get(0)?;
            drop(rows);
            let mut rows = conn
                .query(
                    "SELECT 1 FROM agents WHERE id = ?1 AND project_id = ?2",
                    (target.to_string(), project.0.to_string()),
                )
                .await?;
            anyhow::ensure!(
                rows.next().await?.is_some(),
                "target agent is not in this project"
            );
            drop(rows);

            if let Some(key) = event_key.as_deref() {
                let mut rows = conn
                    .query(
                        "SELECT id, created_at, delivered_at FROM agent_messages
                         WHERE target_agent_id = ?1 AND event_key = ?2 LIMIT 1",
                        (target.to_string(), key),
                    )
                    .await?;
                if let Some(row) = rows.next().await? {
                    return Ok(StoredAgentMessage {
                        id: parse_uuid(&row.get::<String>(0)?)?,
                        source_agent_id: source,
                        target_agent_id: target,
                        source_title,
                        text,
                        kind: kind.to_string(),
                        event_key,
                        created_at: i64_to_u64(row.get(1)?)?,
                        delivered_at: opt_i64(&row, 2)?.map(i64_to_u64).transpose()?,
                    });
                }
            }
            let message = StoredAgentMessage {
                id: Uuid::new_v4(),
                source_agent_id: source,
                target_agent_id: target,
                source_title,
                text,
                kind: kind.to_string(),
                event_key,
                created_at: unix_now(),
                delivered_at: None,
            };
            insert_stored_agent_message_async(&conn, &message).await?;
            Ok(message)
        })
    }

    /// Return exactly one durable response to the source of an earlier request.
    /// The original target is the only agent allowed to answer it, and the
    /// stable event key makes retries from an MCP client idempotent.
    pub fn reply_to_agent_message(
        &self,
        responder: Uuid,
        request_id: Uuid,
        text: &str,
    ) -> Result<StoredAgentMessage> {
        let text = text.trim().to_string();
        anyhow::ensure!(!text.is_empty(), "reply text is empty");
        anyhow::ensure!(
            text.chars().count() <= MAX_AGENT_MESSAGE_CHARS,
            "reply text must be at most {MAX_AGENT_MESSAGE_CHARS} characters"
        );
        retry_database_lock(|| {
            self.rt.block_on(async {
                let conn = self.connect().await?;
                let mut rows = conn
                    .query(
                        "SELECT source_agent_id, target_agent_id, kind
                         FROM agent_messages WHERE id = ?1",
                        [request_id.to_string()],
                    )
                    .await?;
                let Some(request) = rows.next().await? else {
                    return Err(anyhow!("the original agent request no longer exists"));
                };
                let target = parse_uuid(&request.get::<String>(0)?)?;
                let original_target = parse_uuid(&request.get::<String>(1)?)?;
                let original_kind: String = request.get(2)?;
                drop(rows);
                anyhow::ensure!(
                    original_target == responder,
                    "only the requested agent can reply"
                );
                anyhow::ensure!(
                    !matches!(original_kind.as_str(), "reply" | "collision"),
                    "this message does not accept a reply"
                );

                let mut rows = conn
                    .query(
                        "SELECT title FROM agents WHERE id = ?1",
                        [responder.to_string()],
                    )
                    .await?;
                let Some(source) = rows.next().await? else {
                    return Err(anyhow!("replying agent no longer exists"));
                };
                let source_title: String = source.get(0)?;
                drop(rows);

                let event_key = Some(format!("reply:{request_id}"));
                let mut rows = conn
                    .query(
                        "SELECT id, text, created_at, delivered_at FROM agent_messages
                         WHERE target_agent_id = ?1 AND event_key = ?2 LIMIT 1",
                        (target.to_string(), event_key.as_deref().unwrap_or_default()),
                    )
                    .await?;
                if let Some(row) = rows.next().await? {
                    let reply = StoredAgentMessage {
                        id: parse_uuid(&row.get::<String>(0)?)?,
                        source_agent_id: responder,
                        target_agent_id: target,
                        source_title,
                        text: row.get(1)?,
                        kind: "reply".to_string(),
                        event_key,
                        created_at: i64_to_u64(row.get(2)?)?,
                        delivered_at: opt_i64(&row, 3)?.map(i64_to_u64).transpose()?,
                    };
                    drop(rows);
                    conn.execute(
                        "UPDATE agent_messages SET delivered_at = ?2
                         WHERE id = ?1 AND delivered_at IS NULL",
                        params![request_id.to_string(), u64_to_i64(unix_now())?],
                    )
                    .await?;
                    return Ok(reply);
                }
                drop(rows);

                let message = StoredAgentMessage {
                    id: Uuid::new_v4(),
                    source_agent_id: responder,
                    target_agent_id: target,
                    source_title: source_title.clone(),
                    text: text.clone(),
                    kind: "reply".to_string(),
                    event_key,
                    created_at: unix_now(),
                    delivered_at: None,
                };
                let inserted = conn
                    .execute(
                        "INSERT OR IGNORE INTO agent_messages
                         (id, source_agent_id, target_agent_id, text, kind, event_key, created_at, delivered_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                        params![
                            message.id.to_string(),
                            message.source_agent_id.to_string(),
                            message.target_agent_id.to_string(),
                            message.text.as_str(),
                            message.kind.as_str(),
                            message.event_key.clone(),
                            u64_to_i64(message.created_at)?,
                            message.delivered_at.map(u64_to_i64).transpose()?,
                        ],
                    )
                    .await?;
                let reply = if inserted == 0 {
                    // Another caller won the unique event-key race. Return its
                    // durable reply so concurrent retries remain idempotent.
                    let mut rows = conn
                        .query(
                            "SELECT id, text, created_at, delivered_at FROM agent_messages
                             WHERE target_agent_id = ?1 AND event_key = ?2 LIMIT 1",
                            (
                                target.to_string(),
                                message.event_key.as_deref().unwrap_or_default(),
                            ),
                        )
                        .await?;
                    let Some(row) = rows.next().await? else {
                        return Err(anyhow!("the durable agent reply could not be loaded"));
                    };
                    StoredAgentMessage {
                        id: parse_uuid(&row.get::<String>(0)?)?,
                        source_agent_id: responder,
                        target_agent_id: target,
                        source_title,
                        text: row.get(1)?,
                        kind: "reply".to_string(),
                        event_key: message.event_key.clone(),
                        created_at: i64_to_u64(row.get(2)?)?,
                        delivered_at: opt_i64(&row, 3)?.map(i64_to_u64).transpose()?,
                    }
                } else {
                    message
                };
                // A request remains pending until its target has produced the
                // durable reply. This gives delivery at-least-once semantics:
                // closing Choro during hydration or while the turn is queued
                // cannot acknowledge and lose the request. Insert the reply
                // first so a crash between these statements retries safely.
                conn.execute(
                    "UPDATE agent_messages SET delivered_at = ?2
                     WHERE id = ?1 AND delivered_at IS NULL",
                    params![request_id.to_string(), u64_to_i64(unix_now())?],
                )
                .await?;
                Ok(reply)
            })
        })
    }

    pub fn load_pending_agent_messages(&self) -> Result<Vec<StoredAgentMessage>> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            let mut rows = conn
                .query(
                    "SELECT agent_messages.id, agent_messages.source_agent_id,
                            agent_messages.target_agent_id, agents.title,
                            agent_messages.text, agent_messages.kind,
                            agent_messages.event_key, agent_messages.created_at,
                            agent_messages.delivered_at
                     FROM agent_messages
                     INNER JOIN agents ON agents.id = agent_messages.source_agent_id
                     WHERE agent_messages.delivered_at IS NULL
                     ORDER BY agent_messages.created_at ASC",
                    (),
                )
                .await?;
            let mut messages = Vec::new();
            while let Some(row) = rows.next().await? {
                messages.push(StoredAgentMessage {
                    id: parse_uuid(&row.get::<String>(0)?)?,
                    source_agent_id: parse_uuid(&row.get::<String>(1)?)?,
                    target_agent_id: parse_uuid(&row.get::<String>(2)?)?,
                    source_title: row.get(3)?,
                    text: row.get(4)?,
                    kind: row.get(5)?,
                    event_key: opt_text(&row, 6)?,
                    created_at: i64_to_u64(row.get(7)?)?,
                    delivered_at: opt_i64(&row, 8)?.map(i64_to_u64).transpose()?,
                });
            }
            Ok(messages)
        })
    }

    pub fn mark_agent_message_delivered(&self, id: Uuid) -> Result<()> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            conn.execute(
                "UPDATE agent_messages SET delivered_at = ?2
                 WHERE id = ?1 AND delivered_at IS NULL",
                params![id.to_string(), u64_to_i64(unix_now())?],
            )
            .await?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_lock_retries_are_bounded_and_recover() {
        let mut attempts = 0;
        let result = retry_database_lock_with_delay(
            || {
                attempts += 1;
                if attempts < 3 {
                    Err(anyhow!("database is locked"))
                } else {
                    Ok("delivered")
                }
            },
            Duration::ZERO,
        )
        .unwrap();

        assert_eq!(result, "delivered");
        assert_eq!(attempts, 3);
    }

    #[test]
    fn database_lock_retry_does_not_repeat_non_lock_errors() {
        let mut attempts = 0;
        let error = retry_database_lock_with_delay(
            || {
                attempts += 1;
                Err::<(), _>(anyhow!("replying agent no longer exists"))
            },
            Duration::ZERO,
        )
        .unwrap_err();

        assert!(error.to_string().contains("no longer exists"));
        assert_eq!(attempts, 1);
    }
}
