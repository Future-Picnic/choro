//! Cross-agent memory rows.
//!
//! One row = one remembered fact, scoped `"global"` (about the user) or
//! `"project"` (about one repo). The GUI owns the schema; the MCP server
//! writes the same table through `open_existing_default`.

use super::*;

/// Memories are intentionally short facts, not an unbounded document store.
pub const MAX_MEMORY_TEXT_CHARS: usize = 500;
/// Hard ceiling across global and project memories. The prompt has a separate,
/// much smaller rendering budget; this protects the local DB and Settings UI.
pub const MAX_MEMORY_ROWS: usize = 500;

pub(super) fn validate_memory<'a>(
    scope: &str,
    project_id: Option<ProjectId>,
    text: &'a str,
) -> Result<&'a str> {
    let text = text.trim();
    anyhow::ensure!(!text.is_empty(), "memory text is empty");
    anyhow::ensure!(
        text.chars().count() <= MAX_MEMORY_TEXT_CHARS,
        "memory text must be at most {MAX_MEMORY_TEXT_CHARS} characters"
    );
    anyhow::ensure!(
        scope == "global" || scope == "project",
        "memory scope must be \"global\" or \"project\""
    );
    anyhow::ensure!(
        scope != "project" || project_id.is_some(),
        "project-scoped memory needs a project"
    );
    Ok(text)
}

pub(super) async fn insert_memory_async(
    conn: &Connection,
    scope: &str,
    project_id: Option<ProjectId>,
    text: &str,
    source_agent_id: Option<Uuid>,
) -> Result<StoredMemory> {
    let text = validate_memory(scope, project_id, text)?;
    let mut rows = conn.query("SELECT COUNT(*) FROM memories", ()).await?;
    let count: i64 = rows
        .next()
        .await?
        .map(|row| row.get(0))
        .transpose()?
        .unwrap_or(0);
    drop(rows);
    anyhow::ensure!(
        count < MAX_MEMORY_ROWS as i64,
        "memory limit reached ({MAX_MEMORY_ROWS}); delete an old memory first"
    );
    let memory = StoredMemory {
        id: Uuid::new_v4(),
        scope: scope.to_string(),
        project_id: if scope == "project" { project_id } else { None },
        text: text.to_string(),
        enabled: true,
        pinned: false,
        source_agent_id,
        created_at: unix_now(),
        updated_at: unix_now(),
        last_used_at: None,
    };
    conn.execute(
        "INSERT INTO memories
         (id, scope, project_id, text, enabled, pinned, source_agent_id,
          created_at, updated_at, last_used_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            memory.id.to_string(),
            memory.scope.as_str(),
            memory.project_id.map(|id| id.0.to_string()),
            memory.text.as_str(),
            bool_to_i64(memory.enabled),
            bool_to_i64(memory.pinned),
            memory.source_agent_id.map(|id| id.to_string()),
            u64_to_i64(memory.created_at)?,
            u64_to_i64(memory.updated_at)?,
            Option::<i64>::None,
        ],
    )
    .await?;
    Ok(memory)
}

/// Restore an exported row without changing its identity or timestamps.
pub(super) async fn insert_stored_memory_async(
    conn: &Connection,
    memory: &StoredMemory,
) -> Result<()> {
    validate_memory(&memory.scope, memory.project_id, &memory.text)?;
    let enabled = memory.enabled && !(memory.is_global() && memory.source_agent_id.is_some());
    conn.execute(
        "INSERT INTO memories
         (id, scope, project_id, text, enabled, pinned, source_agent_id,
          created_at, updated_at, last_used_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            memory.id.to_string(),
            memory.scope.as_str(),
            memory.project_id.map(|id| id.0.to_string()),
            memory.text.as_str(),
            bool_to_i64(enabled),
            bool_to_i64(memory.pinned),
            memory.source_agent_id.map(|id| id.to_string()),
            u64_to_i64(memory.created_at)?,
            u64_to_i64(memory.updated_at)?,
            memory.last_used_at.map(u64_to_i64).transpose()?,
        ],
    )
    .await?;
    Ok(())
}

/// Memories that apply to a project's sessions: every global row plus the
/// project's own, pinned first, then newest.
pub(super) async fn load_memories_for_project_async(
    conn: &Connection,
    project_id: ProjectId,
) -> Result<Vec<StoredMemory>> {
    let mut rows = conn
        .query(
            "SELECT id, scope, project_id, text, enabled, pinned, source_agent_id,
                    created_at, updated_at, last_used_at
             FROM memories
             WHERE scope = 'global' OR project_id = ?1
             ORDER BY pinned DESC, updated_at DESC",
            [project_id.0.to_string()],
        )
        .await?;
    let mut memories = Vec::new();
    while let Some(row) = rows.next().await? {
        memories.push(memory_from_row(&row)?);
    }
    Ok(memories)
}

/// Every memory, for the Settings list.
pub(super) async fn load_all_memories_async(conn: &Connection) -> Result<Vec<StoredMemory>> {
    let mut rows = conn
        .query(
            "SELECT id, scope, project_id, text, enabled, pinned, source_agent_id,
                    created_at, updated_at, last_used_at
             FROM memories
             ORDER BY pinned DESC, updated_at DESC",
            (),
        )
        .await?;
    let mut memories = Vec::new();
    while let Some(row) = rows.next().await? {
        memories.push(memory_from_row(&row)?);
    }
    Ok(memories)
}

fn memory_from_row(row: &turso::Row) -> Result<StoredMemory> {
    Ok(StoredMemory {
        id: parse_uuid(&row.get::<String>(0)?)?,
        scope: row.get(1)?,
        project_id: opt_text(row, 2)?
            .map(|value| parse_uuid(&value).map(ProjectId))
            .transpose()?,
        text: row.get(3)?,
        enabled: row.get::<i64>(4)? != 0,
        pinned: row.get::<i64>(5)? != 0,
        source_agent_id: opt_text(row, 6)?
            .map(|value| parse_uuid(&value))
            .transpose()?,
        created_at: i64_to_u64(row.get(7)?)?,
        updated_at: i64_to_u64(row.get(8)?)?,
        last_used_at: opt_i64(row, 9)?.map(i64_to_u64).transpose()?,
    })
}
