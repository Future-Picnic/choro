use super::*;

pub(super) async fn insert_chat_message_async(
    conn: &Connection,
    message: &StoredChatMessage,
) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO chat_messages
        (id, agent_id, role, text, sequence, created_at, backend_message_id)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            message.id.to_string(),
            message.agent_id.to_string(),
            message.role.as_str(),
            message.text.as_str(),
            message.sequence,
            u64_to_i64(message.created_at)?,
            message.backend_message_id.clone(),
        ],
    )
    .await?;
    refresh_chat_message_fts_async(conn, message).await?;
    Ok(())
}

pub(super) async fn insert_timeline_event_async(
    conn: &Connection,
    event: &StoredTimelineEvent,
) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO chat_timeline_events
        (id, agent_id, kind, event_key, payload_json, sequence, created_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            event.id.to_string(),
            event.agent_id.to_string(),
            event.kind.as_str(),
            event.event_key.clone(),
            event.payload_json.as_str(),
            event.sequence,
            u64_to_i64(event.created_at)?,
        ],
    )
    .await?;
    Ok(())
}

pub(super) async fn insert_attachment_async(
    conn: &Connection,
    attachment: &StoredAttachment,
) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO attachments
        (id, agent_id, message_id, original_name, mime_type, size_bytes, sha256,
         relative_path, created_at, state)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            attachment.id.to_string(),
            attachment.agent_id.to_string(),
            attachment.message_id.map(|id| id.to_string()),
            attachment.original_name.as_str(),
            attachment.mime_type.clone(),
            u64_to_i64(attachment.size_bytes)?,
            attachment.sha256.as_str(),
            path_to_string(&attachment.relative_path),
            u64_to_i64(attachment.created_at)?,
            attachment.state.as_str(),
        ],
    )
    .await?;
    Ok(())
}

pub(super) async fn next_sequence(conn: &Connection, table: &str, agent_id: Uuid) -> Result<i64> {
    let sql = format!("SELECT COALESCE(MAX(sequence), -1) + 1 FROM {table} WHERE agent_id = ?1");
    let mut rows = conn.query(sql, [agent_id.to_string()]).await?;
    let Some(row) = rows.next().await? else {
        return Ok(0);
    };
    Ok(row.get(0)?)
}

pub(super) async fn load_all_messages_async(conn: &Connection) -> Result<Vec<StoredChatMessage>> {
    let mut rows = conn
        .query(
            "SELECT id, agent_id, role, text, sequence, created_at, backend_message_id
             FROM chat_messages ORDER BY agent_id ASC, sequence ASC",
            (),
        )
        .await?;
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
    Ok(messages)
}

pub(super) async fn load_all_timeline_events_async(
    conn: &Connection,
) -> Result<Vec<StoredTimelineEvent>> {
    let mut rows = conn
        .query(
            "SELECT id, agent_id, kind, event_key, payload_json, sequence, created_at
             FROM chat_timeline_events ORDER BY agent_id ASC, sequence ASC",
            (),
        )
        .await?;
    timeline_events_from_rows(&mut rows).await
}

pub(super) async fn load_timeline_events_async(
    conn: &Connection,
    agent_id: Uuid,
) -> Result<Vec<StoredTimelineEvent>> {
    let mut rows = conn
        .query(
            "SELECT id, agent_id, kind, event_key, payload_json, sequence, created_at
             FROM chat_timeline_events WHERE agent_id = ?1 ORDER BY sequence ASC",
            [agent_id.to_string()],
        )
        .await?;
    timeline_events_from_rows(&mut rows).await
}

pub(super) async fn load_timeline_events_page_async(
    conn: &Connection,
    agent_id: Uuid,
    before_sequence: Option<i64>,
    limit: usize,
) -> Result<StoredTimelinePage> {
    if limit == 0 {
        return Ok(StoredTimelinePage {
            events: Vec::new(),
            oldest_sequence: before_sequence,
            has_more: false,
        });
    }

    // Read one extra event to determine whether another page exists. The
    // sequence cursor keeps this query on idx_chat_timeline_events_agent_sequence
    // instead of making SQLite walk an ever-growing OFFSET.
    let query_limit = i64::try_from(limit.saturating_add(1))?;
    let mut rows = match before_sequence {
        Some(before_sequence) => {
            conn.query(
                "SELECT id, agent_id, kind, event_key, payload_json, sequence, created_at
                 FROM chat_timeline_events
                 WHERE agent_id = ?1 AND sequence < ?2
                 ORDER BY sequence DESC LIMIT ?3",
                params![agent_id.to_string(), before_sequence, query_limit],
            )
            .await?
        }
        None => {
            conn.query(
                "SELECT id, agent_id, kind, event_key, payload_json, sequence, created_at
                 FROM chat_timeline_events
                 WHERE agent_id = ?1
                 ORDER BY sequence DESC LIMIT ?2",
                params![agent_id.to_string(), query_limit],
            )
            .await?
        }
    };
    let mut events = timeline_events_from_rows(&mut rows).await?;
    let has_more = events.len() > limit;
    if has_more {
        events.truncate(limit);
    }
    events.reverse();

    Ok(StoredTimelinePage {
        oldest_sequence: events.first().map(|event| event.sequence),
        events,
        has_more,
    })
}

pub(super) async fn timeline_events_from_rows(
    rows: &mut turso::Rows,
) -> Result<Vec<StoredTimelineEvent>> {
    let mut events = Vec::new();
    while let Some(row) = rows.next().await? {
        events.push(StoredTimelineEvent {
            id: parse_uuid(&row.get::<String>(0)?)?,
            agent_id: parse_uuid(&row.get::<String>(1)?)?,
            kind: row.get(2)?,
            event_key: opt_text(&row, 3)?,
            payload_json: row.get(4)?,
            sequence: row.get(5)?,
            created_at: i64_to_u64(row.get(6)?)?,
        });
    }
    Ok(events)
}

pub(super) async fn load_all_attachments_async(conn: &Connection) -> Result<Vec<StoredAttachment>> {
    let mut rows = conn
        .query(
            "SELECT id, agent_id, message_id, original_name, mime_type, size_bytes, sha256,
             relative_path, created_at, state FROM attachments ORDER BY agent_id ASC, created_at ASC",
            (),
        )
        .await?;
    let mut attachments = Vec::new();
    while let Some(row) = rows.next().await? {
        attachments.push(attachment_from_row(&row)?);
    }
    Ok(attachments)
}
