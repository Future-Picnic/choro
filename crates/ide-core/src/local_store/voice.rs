use super::*;

pub(super) async fn insert_voice_turn_async(
    conn: &Connection,
    mode: &str,
    role: &str,
    text: &str,
    agent_id: Option<Uuid>,
) -> Result<StoredVoiceTurn> {
    anyhow::ensure!(
        matches!(mode, "director" | "dictation" | "project_talk" | "command"),
        "invalid voice mode"
    );
    anyhow::ensure!(
        matches!(role, "user" | "assistant" | "system"),
        "invalid voice role"
    );
    let text = text.trim();
    anyhow::ensure!(!text.is_empty(), "voice turn text is empty");
    let turn = StoredVoiceTurn {
        id: Uuid::new_v4(),
        mode: mode.to_string(),
        role: role.to_string(),
        text: text.to_string(),
        agent_id,
        created_at: unix_now(),
    };
    conn.execute(
        "INSERT INTO voice_turns (id, mode, role, text, agent_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            turn.id.to_string(),
            turn.mode.clone(),
            turn.role.clone(),
            turn.text.clone(),
            turn.agent_id.map(|id| id.to_string()),
            u64_to_i64(turn.created_at)?,
        ],
    )
    .await?;
    Ok(turn)
}

pub(super) async fn load_voice_turns_async(
    conn: &Connection,
    limit: usize,
) -> Result<Vec<StoredVoiceTurn>> {
    let mut rows = conn
        .query(
            "SELECT id, mode, role, text, agent_id, created_at
             FROM voice_turns ORDER BY created_at DESC, rowid DESC LIMIT ?1",
            [i64::try_from(limit.min(1_000))?],
        )
        .await?;
    let mut turns = Vec::new();
    while let Some(row) = rows.next().await? {
        turns.push(StoredVoiceTurn {
            id: parse_uuid(&row.get::<String>(0)?)?,
            mode: row.get(1)?,
            role: row.get(2)?,
            text: row.get(3)?,
            agent_id: opt_text(&row, 4)?
                .map(|value| parse_uuid(&value))
                .transpose()?,
            created_at: i64_to_u64(row.get(5)?)?,
        });
    }
    turns.reverse();
    Ok(turns)
}

pub(super) async fn clear_voice_turns_async(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM voice_turns", ()).await?;
    Ok(())
}

pub(super) async fn prune_voice_turns_async(conn: &Connection, cutoff: u64) -> Result<()> {
    conn.execute(
        "DELETE FROM voice_turns WHERE created_at < ?1",
        [u64_to_i64(cutoff)?],
    )
    .await?;
    Ok(())
}
