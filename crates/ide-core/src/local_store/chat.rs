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

pub(super) async fn upsert_timeline_event_async(
    conn: &Connection,
    agent_id: Uuid,
    kind: String,
    event_key: Option<String>,
    payload_json: String,
    created_at: u64,
) -> Result<StoredTimelineEvent> {
    if let Some(key) = event_key.as_deref() {
        let mut rows = conn
            .query(
                "SELECT id, sequence, payload_json, created_at FROM chat_timeline_events
                 WHERE agent_id = ?1 AND kind = ?2 AND event_key = ?3
                 ORDER BY sequence DESC LIMIT 1",
                (agent_id.to_string(), kind.as_str(), key),
            )
            .await?;
        if let Some(row) = rows.next().await? {
            let id = parse_uuid(&row.get::<String>(0)?)?;
            let sequence = row.get(1)?;
            let existing_payload_json: String = row.get(2)?;
            let existing_created_at = i64_to_u64(row.get(3)?)?;
            drop(rows);
            let keep_existing_payload = should_keep_existing_message_payload(
                kind.as_str(),
                event_key.as_deref(),
                &existing_payload_json,
                &payload_json,
            );
            let event = StoredTimelineEvent {
                id,
                agent_id,
                kind,
                event_key,
                payload_json: if keep_existing_payload {
                    existing_payload_json
                } else {
                    payload_json
                },
                sequence,
                created_at: created_at.max(existing_created_at),
            };
            insert_timeline_event_async(conn, &event).await?;
            return Ok(event);
        }
    }
    let sequence = next_sequence(conn, "chat_timeline_events", agent_id).await?;
    let event = StoredTimelineEvent {
        id: Uuid::new_v4(),
        agent_id,
        kind,
        event_key,
        payload_json,
        sequence,
        created_at,
    };
    insert_timeline_event_async(conn, &event).await?;
    Ok(event)
}

pub(super) async fn replace_chat_file_ledger_async(
    conn: &Connection,
    agent_id: Uuid,
    revision: u64,
    entries: &[StoredChatFileLedgerEntry],
) -> Result<()> {
    let updated_at = entries
        .first()
        .map_or_else(unix_now, |entry| entry.updated_at);
    let applied = conn
        .execute(
            "INSERT INTO chat_file_ledgers (agent_id, revision, updated_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(agent_id) DO UPDATE SET
             revision = excluded.revision,
             updated_at = excluded.updated_at
         WHERE excluded.revision >= chat_file_ledgers.revision",
            (
                agent_id.to_string(),
                u64_to_i64(revision)?,
                u64_to_i64(updated_at)?,
            ),
        )
        .await?;
    if applied == 0 {
        return Ok(());
    }
    conn.execute(
        "DELETE FROM chat_file_ledger WHERE agent_id = ?1",
        [agent_id.to_string()],
    )
    .await?;
    for entry in entries {
        conn.execute(
            "INSERT INTO chat_file_ledger
             (agent_id, path, attribution, additions, deletions, baseline_hash, result_hash,
              baseline_content, result_content, updated_at, counts_unavailable, segments_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                agent_id.to_string(),
                path_to_string(&entry.path),
                if entry.observed { "observed" } else { "exact" },
                u64_to_i64(entry.additions as u64)?,
                u64_to_i64(entry.deletions as u64)?,
                entry.baseline_hash.clone(),
                entry.result_hash.clone(),
                entry.baseline_content.clone(),
                entry.result_content.clone(),
                u64_to_i64(entry.updated_at)?,
                i64::from(entry.counts_unavailable),
                entry.segments_json.clone(),
            ],
        )
        .await?;
    }
    Ok(())
}

pub(super) async fn load_chat_file_ledger_async(
    conn: &Connection,
    agent_id: Uuid,
) -> Result<Option<StoredChatFileLedger>> {
    let mut ledger_rows = conn
        .query(
            "SELECT revision, updated_at FROM chat_file_ledgers WHERE agent_id = ?1 LIMIT 1",
            [agent_id.to_string()],
        )
        .await?;
    let Some(ledger_row) = ledger_rows.next().await? else {
        return Ok(None);
    };
    let revision = i64_to_u64(ledger_row.get(0)?)?;
    let updated_at = i64_to_u64(ledger_row.get(1)?)?;
    drop(ledger_rows);
    let mut rows = conn
        .query(
            "SELECT path, attribution, additions, deletions, baseline_hash, result_hash,
                    baseline_content, result_content, updated_at, counts_unavailable, segments_json
             FROM chat_file_ledger WHERE agent_id = ?1 ORDER BY path ASC",
            [agent_id.to_string()],
        )
        .await?;
    let mut entries = Vec::new();
    while let Some(row) = rows.next().await? {
        let additions = i64_to_u64(row.get(2)?)? as usize;
        let deletions = i64_to_u64(row.get(3)?)? as usize;
        entries.push(StoredChatFileLedgerEntry {
            agent_id,
            path: PathBuf::from(row.get::<String>(0)?),
            observed: row.get::<String>(1)? == "observed",
            additions,
            deletions,
            baseline_hash: opt_text(&row, 4)?,
            result_hash: opt_text(&row, 5)?,
            baseline_content: opt_text(&row, 6)?,
            result_content: opt_text(&row, 7)?,
            updated_at: i64_to_u64(row.get(8)?)?,
            counts_unavailable: row.get::<i64>(9)? != 0,
            segments_json: row.get::<String>(10)?,
        });
    }
    Ok(Some(StoredChatFileLedger {
        agent_id,
        revision,
        updated_at,
        entries,
    }))
}

pub(super) async fn load_all_chat_file_ledgers_async(
    conn: &Connection,
) -> Result<Vec<StoredChatFileLedger>> {
    let mut rows = conn
        .query(
            "SELECT agent_id, revision, updated_at FROM chat_file_ledgers ORDER BY agent_id ASC",
            (),
        )
        .await?;
    let mut markers = Vec::new();
    while let Some(row) = rows.next().await? {
        markers.push((
            parse_uuid(&row.get::<String>(0)?)?,
            i64_to_u64(row.get(1)?)?,
            i64_to_u64(row.get(2)?)?,
        ));
    }
    drop(rows);
    let mut ledgers = Vec::new();
    for (agent_id, revision, updated_at) in markers {
        if let Some(mut ledger) = load_chat_file_ledger_async(conn, agent_id).await? {
            ledger.revision = revision;
            ledger.updated_at = updated_at;
            ledgers.push(ledger);
        }
    }
    Ok(ledgers)
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

pub(super) async fn search_timeline_message_candidates_page_async(
    conn: &Connection,
    agent_id: Uuid,
    folded_query: &str,
    search_text_version: u64,
    before_sequence: Option<i64>,
    limit: usize,
) -> Result<StoredTimelinePage> {
    if folded_query.is_empty() || limit == 0 {
        return Ok(StoredTimelinePage {
            events: Vec::new(),
            oldest_sequence: None,
            has_more: false,
        });
    }

    // User rows delimit turns and are intentionally retained so the caller can
    // suppress assistant output belonging to hidden maintenance turns. Current
    // assistant rows use their pre-folded, display-equivalent text as the fast
    // path. Older/future payload versions are returned as candidates so the
    // caller can normalize them without a destructive data migration.
    let query_limit = i64::try_from(limit.saturating_add(1))?;
    let search_text_version = i64::try_from(search_text_version)?;
    let mut rows = match before_sequence {
        Some(before_sequence) => {
            conn.query(
                "SELECT id, agent_id, kind, event_key, payload_json, sequence, created_at
                 FROM chat_timeline_events
                 WHERE agent_id = ?1
                   AND sequence < ?2
                   AND kind = 'message'
                   AND (
                        json_extract(payload_json, '$.role') = 'user'
                        OR (
                            json_extract(payload_json, '$.role') = 'assistant'
                            AND (
                                COALESCE(CAST(json_extract(payload_json, '$.search_text_version') AS INTEGER), 0) != ?3
                                OR instr(COALESCE(json_extract(payload_json, '$.search_text'), ''), ?4) > 0
                            )
                        )
                   )
                 ORDER BY sequence DESC
                 LIMIT ?5",
                params![
                    agent_id.to_string(),
                    before_sequence,
                    search_text_version,
                    folded_query,
                    query_limit
                ],
            )
            .await?
        }
        None => {
            conn.query(
                "SELECT id, agent_id, kind, event_key, payload_json, sequence, created_at
                 FROM chat_timeline_events
                 WHERE agent_id = ?1
                   AND kind = 'message'
                   AND (
                        json_extract(payload_json, '$.role') = 'user'
                        OR (
                            json_extract(payload_json, '$.role') = 'assistant'
                            AND (
                                COALESCE(CAST(json_extract(payload_json, '$.search_text_version') AS INTEGER), 0) != ?2
                                OR instr(COALESCE(json_extract(payload_json, '$.search_text'), ''), ?3) > 0
                            )
                        )
                   )
                 ORDER BY sequence DESC
                 LIMIT ?4",
                params![
                    agent_id.to_string(),
                    search_text_version,
                    folded_query,
                    query_limit
                ],
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
