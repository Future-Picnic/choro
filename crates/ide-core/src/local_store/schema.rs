use super::*;

pub(super) async fn run_migrations(conn: &Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            applied_at INTEGER NOT NULL
        )",
        (),
    )
    .await?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )",
        (),
    )
    .await?;

    let current = schema_version(conn).await?;
    if current > STORE_SCHEMA_VERSION {
        return Err(anyhow!(
            "local store schema version {current} is newer than this Choro build supports ({STORE_SCHEMA_VERSION})"
        ));
    }
    migrate_turso_fts_storage(conn).await?;
    if current == STORE_SCHEMA_VERSION {
        super::agent_changes::ensure_schema(conn).await?;
        super::code_review::ensure_schema(conn).await?;
        // Development and preview builds can share the same local database while
        // independently assigning a schema version. If another build recorded
        // version 19 before the Penpot tables existed, the version alone is not
        // enough to prove this feature schema is present. The statements are all
        // idempotent, so repair the feature schema on open without touching data.
        ensure_penpot_schema(conn).await?;
        ensure_penpot_source_columns(conn).await?;
        ensure_penpot_conversation_model_columns(conn).await?;
        super::remote::ensure_schema(conn).await?;
        ensure_voice_schema(conn).await?;
        ensure_agent_repository_column(conn).await?;
        ensure_git_workflow_schema(conn).await?;
        ensure_brain_schema(conn).await?;
        ensure_verification_closed_column(conn).await?;
        ensure_chat_file_ledger_schema(conn).await?;
        ensure_agent_origin_column(conn).await?;
        ensure_pending_project_script_presets_schema(conn).await?;
        ensure_orbit_schema(conn).await?;
        ensure_quick_ask_schema(conn).await?;
        execute_transaction(conn, |conn| {
            Box::pin(async move { super::delegation::ensure_delegation_schema_inner(conn).await })
        })
        .await?;
        return Ok(());
    }
    if current < 1 {
        execute_transaction(conn, |conn| {
            Box::pin(async move {
                for statement in SCHEMA_V1 {
                    conn.execute(statement, ()).await?;
                }
                record_schema_version(conn, 1).await?;
                Ok(())
            })
        })
        .await?;
    }
    if current < 2 {
        execute_transaction(conn, |conn| {
            Box::pin(async move {
                if !column_exists(conn, "chat_timeline_events", "event_key").await? {
                    conn.execute(
                        "ALTER TABLE chat_timeline_events ADD COLUMN event_key TEXT",
                        (),
                    )
                    .await?;
                }
                for statement in SCHEMA_V2 {
                    conn.execute(statement, ()).await?;
                }
                record_schema_version(conn, 2).await?;
                Ok(())
            })
        })
        .await?;
    }
    if current < 3 {
        execute_transaction(conn, |conn| {
            Box::pin(async move {
                for statement in SCHEMA_V3 {
                    conn.execute(statement, ()).await?;
                }
                record_schema_version(conn, 3).await?;
                Ok(())
            })
        })
        .await?;
    }
    if current < 4 {
        execute_transaction(conn, |conn| {
            Box::pin(async move {
                for statement in SCHEMA_V4 {
                    conn.execute(statement, ()).await?;
                }
                record_schema_version(conn, 4).await?;
                Ok(())
            })
        })
        .await?;
    }
    if current < 5 {
        execute_transaction(conn, |conn| {
            Box::pin(async move {
                for statement in SCHEMA_V5 {
                    conn.execute(statement, ()).await?;
                }
                record_schema_version(conn, 5).await?;
                Ok(())
            })
        })
        .await?;
    }
    if current < 6 {
        execute_transaction(conn, |conn| {
            Box::pin(async move {
                if !column_exists(conn, "project_task_tracker_connections", "assignee_filter")
                    .await?
                {
                    conn.execute(
                        "ALTER TABLE project_task_tracker_connections
                         ADD COLUMN assignee_filter TEXT",
                        (),
                    )
                    .await?;
                }
                record_schema_version(conn, 6).await?;
                Ok(())
            })
        })
        .await?;
    }
    if current < 7 {
        execute_transaction(conn, |conn| {
            Box::pin(async move {
                if !column_exists(
                    conn,
                    "project_task_tracker_connections",
                    "assignee_account_id",
                )
                .await?
                {
                    conn.execute(
                        "ALTER TABLE project_task_tracker_connections
                         ADD COLUMN assignee_account_id TEXT",
                        (),
                    )
                    .await?;
                }
                if !column_exists(
                    conn,
                    "project_task_tracker_connections",
                    "assignee_display_name",
                )
                .await?
                {
                    conn.execute(
                        "ALTER TABLE project_task_tracker_connections
                         ADD COLUMN assignee_display_name TEXT",
                        (),
                    )
                    .await?;
                }
                record_schema_version(conn, 7).await?;
                Ok(())
            })
        })
        .await?;
    }
    if current < 8 {
        execute_transaction(conn, |conn| {
            Box::pin(async move {
                for (column, ty) in [
                    ("source_id", "TEXT"),
                    ("source_name", "TEXT"),
                    ("source_kind", "TEXT"),
                    ("provider_config_json", "TEXT NOT NULL DEFAULT '{}'"),
                    ("filters_json", "TEXT NOT NULL DEFAULT '{}'"),
                ] {
                    if !column_exists(conn, "project_task_tracker_connections", column).await? {
                        conn.execute(
                            format!(
                                "ALTER TABLE project_task_tracker_connections ADD COLUMN {column} {ty}"
                            ),
                            (),
                        )
                        .await?;
                    }
                }
                conn.execute(
                    "UPDATE project_task_tracker_connections
                     SET source_id = COALESCE(source_id, CAST(board_id AS TEXT)),
                         source_name = COALESCE(source_name, board_name),
                         source_kind = COALESCE(source_kind, 'board')
                     WHERE provider = 'jira'",
                    (),
                )
                .await?;
                for statement in SCHEMA_V8 {
                    conn.execute(statement, ()).await?;
                }
                record_schema_version(conn, 8).await?;
                Ok(())
            })
        })
        .await?;
    }
    if current < 9 {
        execute_transaction(conn, |conn| {
            Box::pin(async move {
                for statement in SCHEMA_V9 {
                    conn.execute(statement, ()).await?;
                }
                record_schema_version(conn, 9).await?;
                Ok(())
            })
        })
        .await?;
    }
    if current < 10 {
        execute_transaction(conn, |conn| {
            Box::pin(async move {
                conn.execute(
                    "UPDATE agents
                     SET source_doc = 'choro_docs' || substr(source_doc, length('my_ide_docs') + 1)
                     WHERE source_doc = 'my_ide_docs' OR source_doc LIKE 'my_ide_docs/%'",
                    (),
                )
                .await?;
                conn.execute(
                    "INSERT OR IGNORE INTO agent_linked_docs (agent_id, path, sort_order)
                     SELECT agent_id,
                            'choro_docs' || substr(path, length('my_ide_docs') + 1),
                            sort_order
                     FROM agent_linked_docs
                     WHERE path = 'my_ide_docs' OR path LIKE 'my_ide_docs/%'",
                    (),
                )
                .await?;
                conn.execute(
                    "DELETE FROM agent_linked_docs
                     WHERE path = 'my_ide_docs' OR path LIKE 'my_ide_docs/%'",
                    (),
                )
                .await?;
                record_schema_version(conn, 10).await?;
                Ok(())
            })
        })
        .await?;
    }
    if current < 11 {
        execute_transaction(conn, |conn| {
            Box::pin(async move {
                if !column_exists(conn, "project_db_connections", "provider").await? {
                    conn.execute(
                        "ALTER TABLE project_db_connections
                         ADD COLUMN provider TEXT NOT NULL DEFAULT 'mongodb'",
                        (),
                    )
                    .await?;
                }
                if !column_exists(conn, "project_db_connections", "read_only").await? {
                    conn.execute(
                        "ALTER TABLE project_db_connections
                         ADD COLUMN read_only INTEGER NOT NULL DEFAULT 0",
                        (),
                    )
                    .await?;
                }
                record_schema_version(conn, 11).await?;
                Ok(())
            })
        })
        .await?;
    }
    if current < 12 {
        execute_transaction(conn, |conn| {
            Box::pin(async move {
                for column in [
                    "external_model_id TEXT",
                    "external_model_label TEXT",
                    "external_model_variants TEXT NOT NULL DEFAULT '[]'",
                ] {
                    let name = column.split_whitespace().next().unwrap_or_default();
                    if !column_exists(conn, "agents", name).await? {
                        conn.execute(format!("ALTER TABLE agents ADD COLUMN {column}"), ())
                            .await?;
                    }
                }
                record_schema_version(conn, 12).await?;
                Ok(())
            })
        })
        .await?;
    }
    if current < 13 {
        execute_transaction(conn, |conn| {
            Box::pin(async move {
                for statement in SCHEMA_V13 {
                    conn.execute(statement, ()).await?;
                }
                record_schema_version(conn, 13).await?;
                Ok(())
            })
        })
        .await?;
    }
    if current < 14 {
        execute_transaction(conn, |conn| {
            Box::pin(async move {
                for statement in SCHEMA_V14 {
                    conn.execute(statement, ()).await?;
                }
                record_schema_version(conn, 14).await?;
                Ok(())
            })
        })
        .await?;
    }
    if current < 15 {
        execute_transaction(conn, |conn| {
            Box::pin(async move {
                for column in [
                    "lane_path TEXT",
                    "solo_branch TEXT",
                    "solo_base_branch TEXT",
                    "lane_profile TEXT",
                ] {
                    let name = column.split_whitespace().next().unwrap_or_default();
                    if !column_exists(conn, "agents", name).await? {
                        conn.execute(format!("ALTER TABLE agents ADD COLUMN {column}"), ())
                            .await?;
                    }
                }
                record_schema_version(conn, 15).await?;
                Ok(())
            })
        })
        .await?;
    }
    if current < 16 {
        execute_transaction(conn, |conn| {
            Box::pin(async move {
                for statement in SCHEMA_V16 {
                    conn.execute(statement, ()).await?;
                }
                record_schema_version(conn, 16).await?;
                Ok(())
            })
        })
        .await?;
    }
    if current < 17 {
        execute_transaction(conn, |conn| {
            Box::pin(async move {
                if !column_exists(conn, "agents", "solo_rejoined_branch").await? {
                    conn.execute(
                        "ALTER TABLE agents ADD COLUMN solo_rejoined_branch TEXT",
                        (),
                    )
                    .await?;
                }
                // Older development builds allowed agents to create global
                // memories. Quarantine those rows; only Settings-authored
                // globals (no source agent) remain active.
                conn.execute(
                    "UPDATE memories SET enabled = 0
                     WHERE scope = 'global' AND source_agent_id IS NOT NULL",
                    (),
                )
                .await?;
                record_schema_version(conn, 17).await?;
                Ok(())
            })
        })
        .await?;
    }
    if current < 18 {
        execute_transaction(conn, |conn| {
            Box::pin(async move {
                if !column_exists(conn, "agents", "verification_completed_at").await? {
                    conn.execute(
                        "ALTER TABLE agents ADD COLUMN verification_completed_at INTEGER",
                        (),
                    )
                    .await?;
                }
                record_schema_version(conn, 18).await?;
                Ok(())
            })
        })
        .await?;
    }
    if current < 19 {
        execute_transaction(conn, |conn| {
            Box::pin(async move {
                for statement in SCHEMA_V19 {
                    conn.execute(statement, ()).await?;
                }
                record_schema_version(conn, 19).await?;
                Ok(())
            })
        })
        .await?;
    }
    if current < 20 {
        execute_transaction(conn, |conn| {
            Box::pin(async move {
                ensure_penpot_source_columns_inner(conn).await?;
                record_schema_version(conn, 20).await?;
                Ok(())
            })
        })
        .await?;
    }
    if current < 21 {
        execute_transaction(conn, |conn| {
            Box::pin(async move {
                // Version 21 moved Preview control to authenticated local IPC.
                // Preserve the Penpot feature schema when this worktree opens a
                // store that was upgraded by the newer Preview build.
                for statement in SCHEMA_V19 {
                    conn.execute(statement, ()).await?;
                }
                ensure_penpot_source_columns_inner(conn).await?;
                conn.execute("DROP TABLE IF EXISTS preview_control_commands", ())
                    .await?;
                record_schema_version(conn, 21).await?;
                Ok(())
            })
        })
        .await?;
    }
    if current < 22 {
        execute_transaction(conn, |conn| {
            Box::pin(async move {
                ensure_penpot_conversation_model_columns_inner(conn).await?;
                record_schema_version(conn, 22).await?;
                Ok(())
            })
        })
        .await?;
    }
    if current < 23 {
        execute_transaction(conn, |conn| {
            Box::pin(async move {
                for statement in SCHEMA_V23 {
                    conn.execute(statement, ()).await?;
                }
                ensure_agent_repository_column_inner(conn).await?;
                record_schema_version(conn, 23).await?;
                Ok(())
            })
        })
        .await?;
    }
    if current < 24 {
        execute_transaction(conn, |conn| {
            Box::pin(async move {
                migrate_voice_schema_v24_inner(conn).await?;
                record_schema_version(conn, 24).await?;
                Ok(())
            })
        })
        .await?;
    }
    if current < 25 {
        execute_transaction(conn, |conn| {
            Box::pin(async move {
                ensure_agent_repository_column_inner(conn).await?;
                record_schema_version(conn, 25).await?;
                Ok(())
            })
        })
        .await?;
    }
    if current < 26 {
        execute_transaction(conn, |conn| {
            Box::pin(async move {
                ensure_git_workflow_schema_inner(conn).await?;
                record_schema_version(conn, 26).await?;
                Ok(())
            })
        })
        .await?;
    }
    if current < 27 {
        execute_transaction(conn, |conn| {
            Box::pin(async move {
                ensure_brain_schema_inner(conn).await?;
                record_schema_version(conn, 27).await?;
                Ok(())
            })
        })
        .await?;
    }
    if current < 28 {
        execute_transaction(conn, |conn| {
            Box::pin(async move {
                ensure_brain_schema_inner(conn).await?;
                record_schema_version(conn, 28).await?;
                Ok(())
            })
        })
        .await?;
    }
    if current < 29 {
        execute_transaction(conn, |conn| {
            Box::pin(async move {
                ensure_verification_closed_column_inner(conn).await?;
                record_schema_version(conn, 29).await?;
                Ok(())
            })
        })
        .await?;
    }
    if current < 30 {
        execute_transaction(conn, |conn| {
            Box::pin(async move {
                ensure_chat_file_ledger_schema_inner(conn).await?;
                record_schema_version(conn, 30).await?;
                Ok(())
            })
        })
        .await?;
    }
    if current < 31 {
        execute_transaction(conn, |conn| {
            Box::pin(async move {
                ensure_agent_origin_column_inner(conn).await?;
                record_schema_version(conn, 31).await?;
                Ok(())
            })
        })
        .await?;
    }
    if current < 32 {
        execute_transaction(conn, |conn| {
            Box::pin(async move {
                ensure_pending_project_script_presets_schema_inner(conn).await?;
                record_schema_version(conn, 32).await?;
                Ok(())
            })
        })
        .await?;
    }
    if current < 33 {
        execute_transaction(conn, |conn| {
            Box::pin(async move {
                ensure_orbit_schema_inner(conn).await?;
                record_schema_version(conn, 33).await?;
                Ok(())
            })
        })
        .await?;
    }
    if current < 34 {
        execute_transaction(conn, |conn| {
            Box::pin(async move {
                ensure_quick_ask_schema_inner(conn).await?;
                record_schema_version(conn, 34).await?;
                Ok(())
            })
        })
        .await?;
    }
    if current < 35 {
        execute_transaction(conn, |conn| {
            Box::pin(async move {
                super::delegation::ensure_delegation_schema_inner(conn).await?;
                record_schema_version(conn, 35).await?;
                Ok(())
            })
        })
        .await?;
    }
    if current < 36 {
        execute_transaction(conn, |conn| Box::pin(async move {
            super::remote::ensure_schema(conn).await?;
            record_schema_version(conn, 36).await?;
            Ok(())
        })).await?;
    }
    if current < 37 {
        execute_transaction(conn, |conn| Box::pin(async move {
            super::agent_changes::ensure_schema(conn).await?;
            ensure_chat_file_ledger_schema_inner(conn).await?;
            record_schema_version(conn, 37).await?;
            Ok(())
        })).await?;
    }
    if current < 38 {
        execute_transaction(conn, |conn| Box::pin(async move {
            super::agent_changes::ensure_schema(conn).await?;
            ensure_chat_file_ledger_schema_inner(conn).await?;
            record_schema_version(conn, 38).await?;
            Ok(())
        })).await?;
    }
    if current < 39 {
        execute_transaction(conn, |conn| Box::pin(async move {
            super::code_review::ensure_schema(conn).await?;
            record_schema_version(conn, 39).await?;
            Ok(())
        })).await?;
    }

    Ok(())
}

/// Turso 0.8 cannot read the FTS index storage produced by 0.7. Rebuild only
/// Choro's derived indexes, preserving their source rows. Track the engine
/// format separately from the application's schema migrations, since an
/// existing database can already have the current application schema.
async fn migrate_turso_fts_storage(conn: &Connection) -> Result<()> {
    const KEY: &str = "turso_fts_storage_format";
    const FORMAT: &str = "0.8.0-pre.13";
    if get_meta(conn, KEY).await?.as_deref() == Some(FORMAT) {
        return Ok(());
    }
    execute_transaction(conn, |conn| {
        Box::pin(async move {
            // Recheck after reserving the writer in case another opener ran it.
            if get_meta(conn, KEY).await?.as_deref() == Some(FORMAT) {
                return Ok(());
            }
            for (index, table, columns) in [
                (
                    "idx_agent_search_fts_text",
                    "agent_search_fts",
                    "title, summary_text",
                ),
                ("idx_chat_messages_fts_text", "chat_messages_fts", "text"),
            ] {
                if table_exists(conn, table).await? {
                    conn.execute(format!("DROP INDEX IF EXISTS {index}"), ())
                        .await?;
                    conn.execute(
                        format!("CREATE INDEX {index} ON {table} USING fts ({columns})"),
                        (),
                    )
                    .await?;
                }
            }
            set_meta(conn, KEY, FORMAT).await
        })
    })
    .await
}

async fn ensure_quick_ask_schema(conn: &Connection) -> Result<()> {
    execute_transaction(conn, |conn| {
        Box::pin(async move { ensure_quick_ask_schema_inner(conn).await })
    })
    .await
}

async fn ensure_quick_ask_schema_inner(conn: &Connection) -> Result<()> {
    for statement in SCHEMA_V34 {
        conn.execute(statement, ()).await?;
    }
    Ok(())
}

async fn ensure_orbit_schema(conn: &Connection) -> Result<()> {
    execute_transaction(conn, |conn| {
        Box::pin(async move { ensure_orbit_schema_inner(conn).await })
    })
    .await
}

async fn ensure_orbit_schema_inner(conn: &Connection) -> Result<()> {
    for statement in SCHEMA_V33 {
        conn.execute(statement, ()).await?;
    }
    Ok(())
}

async fn ensure_pending_project_script_presets_schema(conn: &Connection) -> Result<()> {
    execute_transaction(conn, |conn| {
        Box::pin(async move { ensure_pending_project_script_presets_schema_inner(conn).await })
    })
    .await
}

async fn ensure_pending_project_script_presets_schema_inner(conn: &Connection) -> Result<()> {
    for statement in SCHEMA_V32 {
        conn.execute(statement, ()).await?;
    }
    Ok(())
}

async fn ensure_agent_origin_column(conn: &Connection) -> Result<()> {
    execute_transaction(conn, |conn| {
        Box::pin(async move { ensure_agent_origin_column_inner(conn).await })
    })
    .await
}

async fn ensure_agent_origin_column_inner(conn: &Connection) -> Result<()> {
    if !column_exists(conn, "agents", "origin_json").await? {
        conn.execute("ALTER TABLE agents ADD COLUMN origin_json TEXT", ())
            .await?;
    }
    Ok(())
}

async fn ensure_chat_file_ledger_schema(conn: &Connection) -> Result<()> {
    execute_transaction(conn, |conn| {
        Box::pin(async move { ensure_chat_file_ledger_schema_inner(conn).await })
    })
    .await
}

async fn ensure_chat_file_ledger_schema_inner(conn: &Connection) -> Result<()> {
    for statement in SCHEMA_V30 {
        conn.execute(statement, ()).await?;
    }
    if !column_exists(conn, "chat_file_ledgers", "revision").await? {
        conn.execute(
            "ALTER TABLE chat_file_ledgers ADD COLUMN revision INTEGER NOT NULL DEFAULT 0",
            (),
        )
        .await?;
    }
    for (column, definition) in [
        ("baseline_content", "TEXT"),
        ("result_content", "TEXT"),
        ("counts_unavailable", "INTEGER NOT NULL DEFAULT 0"),
        ("segments_json", "TEXT NOT NULL DEFAULT '[]'"),
    ] {
        if !column_exists(conn, "chat_file_ledger", column).await? {
            conn.execute(
                format!("ALTER TABLE chat_file_ledger ADD COLUMN {column} {definition}"),
                (),
            )
            .await?;
        }
    }
    Ok(())
}

async fn ensure_brain_schema(conn: &Connection) -> Result<()> {
    execute_transaction(conn, |conn| {
        Box::pin(async move { ensure_brain_schema_inner(conn).await })
    })
    .await
}

async fn ensure_brain_schema_inner(conn: &Connection) -> Result<()> {
    for statement in SCHEMA_V27 {
        conn.execute(statement, ()).await?;
    }
    if !column_exists(conn, "agent_summaries", "outcome_text").await? {
        conn.execute(
            "ALTER TABLE agent_summaries ADD COLUMN outcome_text TEXT",
            (),
        )
        .await?;
    }
    // Legacy FTS projection tables remain for schema compatibility, but are
    // intentionally not populated. Brain search reads the source tables after
    // indexed Turso writes caused process-ending panics in production.
    Ok(())
}

async fn ensure_git_workflow_schema(conn: &Connection) -> Result<()> {
    execute_transaction(conn, |conn| {
        Box::pin(async move { ensure_git_workflow_schema_inner(conn).await })
    })
    .await
}

async fn ensure_git_workflow_schema_inner(conn: &Connection) -> Result<()> {
    for statement in SCHEMA_V26 {
        conn.execute(statement, ()).await?;
    }
    Ok(())
}

async fn ensure_voice_schema(conn: &Connection) -> Result<()> {
    execute_transaction(conn, |conn| {
        Box::pin(async move { ensure_voice_schema_inner(conn).await })
    })
    .await
}

async fn ensure_voice_schema_inner(conn: &Connection) -> Result<()> {
    for statement in SCHEMA_V24 {
        conn.execute(statement, ()).await?;
    }
    Ok(())
}

async fn migrate_voice_schema_v24_inner(conn: &Connection) -> Result<()> {
    for statement in SCHEMA_V23 {
        conn.execute(statement, ()).await?;
    }
    conn.execute("DROP INDEX IF EXISTS idx_voice_turns_created", ())
        .await?;
    conn.execute(
        "ALTER TABLE voice_turns RENAME TO voice_turns_v23_backup",
        (),
    )
    .await?;
    for statement in SCHEMA_V24 {
        conn.execute(statement, ()).await?;
    }
    conn.execute(
        "INSERT INTO voice_turns (id, mode, role, text, agent_id, created_at)
         SELECT id, mode, role, text, agent_id, created_at FROM voice_turns_v23_backup",
        (),
    )
    .await?;
    conn.execute("DROP TABLE voice_turns_v23_backup", ())
        .await?;
    Ok(())
}

async fn ensure_agent_repository_column(conn: &Connection) -> Result<()> {
    execute_transaction(conn, |conn| {
        Box::pin(async move { ensure_agent_repository_column_inner(conn).await })
    })
    .await
}

async fn ensure_agent_repository_column_inner(conn: &Connection) -> Result<()> {
    if !column_exists(conn, "agents", "repository_path").await? {
        conn.execute("ALTER TABLE agents ADD COLUMN repository_path TEXT", ())
            .await?;
    }
    Ok(())
}

async fn ensure_verification_closed_column(conn: &Connection) -> Result<()> {
    execute_transaction(conn, |conn| {
        Box::pin(async move { ensure_verification_closed_column_inner(conn).await })
    })
    .await
}

async fn ensure_verification_closed_column_inner(conn: &Connection) -> Result<()> {
    if !column_exists(conn, "agents", "verification_completed_at").await? {
        conn.execute(
            "ALTER TABLE agents ADD COLUMN verification_completed_at INTEGER",
            (),
        )
        .await?;
    }
    if !column_exists(conn, "agents", "verification_closed").await? {
        conn.execute(
            "ALTER TABLE agents ADD COLUMN verification_closed INTEGER NOT NULL DEFAULT 0",
            (),
        )
        .await?;
    }
    // Completion was the original terminal verification state. Promote those
    // records so the new hard gate is correct immediately after migration.
    conn.execute(
        "UPDATE agents SET verification_closed = 1
         WHERE verification_completed_at IS NOT NULL AND verification_closed = 0",
        (),
    )
    .await?;
    Ok(())
}

async fn ensure_penpot_schema(conn: &Connection) -> Result<()> {
    execute_transaction(conn, |conn| {
        Box::pin(async move {
            for statement in SCHEMA_V19 {
                conn.execute(statement, ()).await?;
            }
            Ok(())
        })
    })
    .await
}

async fn ensure_penpot_source_columns(conn: &Connection) -> Result<()> {
    execute_transaction(conn, |conn| {
        Box::pin(async move { ensure_penpot_source_columns_inner(conn).await })
    })
    .await
}

async fn ensure_penpot_conversation_model_columns(conn: &Connection) -> Result<()> {
    execute_transaction(conn, |conn| {
        Box::pin(async move { ensure_penpot_conversation_model_columns_inner(conn).await })
    })
    .await
}

async fn ensure_penpot_conversation_model_columns_inner(conn: &Connection) -> Result<()> {
    for (column, ty) in [
        ("external_model_id", "TEXT"),
        ("external_model_label", "TEXT"),
        ("external_model_variants", "TEXT NOT NULL DEFAULT '[]'"),
    ] {
        if !column_exists(conn, "penpot_design_conversations", column).await? {
            conn.execute(
                format!("ALTER TABLE penpot_design_conversations ADD COLUMN {column} {ty}"),
                (),
            )
            .await?;
        }
    }
    Ok(())
}

async fn ensure_penpot_source_columns_inner(conn: &Connection) -> Result<()> {
    if !column_exists(conn, "penpot_designs", "source_doc").await? {
        conn.execute("ALTER TABLE penpot_designs ADD COLUMN source_doc TEXT", ())
            .await?;
    }
    if !column_exists(conn, "penpot_designs", "source_task_json").await? {
        conn.execute(
            "ALTER TABLE penpot_designs ADD COLUMN source_task_json TEXT",
            (),
        )
        .await?;
    }
    Ok(())
}

pub(super) const SCHEMA_V23: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS voice_turns (
        id TEXT PRIMARY KEY,
        mode TEXT NOT NULL CHECK(mode IN ('director', 'dictation')),
        role TEXT NOT NULL CHECK(role IN ('user', 'assistant', 'system')),
        text TEXT NOT NULL,
        agent_id TEXT,
        created_at INTEGER NOT NULL
    )",
    "CREATE INDEX IF NOT EXISTS idx_voice_turns_created
        ON voice_turns(created_at DESC)",
];

const SCHEMA_V24: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS voice_turns (
        id TEXT PRIMARY KEY,
        mode TEXT NOT NULL CHECK(mode IN ('director', 'dictation', 'project_talk', 'command')),
        role TEXT NOT NULL CHECK(role IN ('user', 'assistant', 'system')),
        text TEXT NOT NULL,
        agent_id TEXT,
        created_at INTEGER NOT NULL
    )",
    "CREATE INDEX IF NOT EXISTS idx_voice_turns_created
        ON voice_turns(created_at DESC)",
];

const SCHEMA_V19: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS penpot_connections (
        id TEXT PRIMARY KEY,
        instance_url TEXT NOT NULL,
        mcp_url TEXT NOT NULL,
        profile_id TEXT,
        profile_email TEXT,
        default_team_id TEXT,
        default_project_id TEXT,
        is_active INTEGER NOT NULL DEFAULT 0,
        verified_at INTEGER,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    )",
    "CREATE UNIQUE INDEX IF NOT EXISTS idx_penpot_connections_one_active
        ON penpot_connections(is_active) WHERE is_active = 1",
    "CREATE TABLE IF NOT EXISTS project_penpot_bindings (
        project_id TEXT NOT NULL,
        connection_id TEXT NOT NULL,
        penpot_team_id TEXT NOT NULL,
        penpot_project_id TEXT NOT NULL,
        selected_design_id TEXT,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        PRIMARY KEY(project_id, connection_id),
        FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
        FOREIGN KEY(connection_id) REFERENCES penpot_connections(id) ON DELETE RESTRICT
    )",
    "CREATE TABLE IF NOT EXISTS penpot_designs (
        id TEXT PRIMARY KEY,
        project_id TEXT NOT NULL,
        connection_id TEXT NOT NULL,
        penpot_file_id TEXT NOT NULL,
        penpot_project_id TEXT NOT NULL,
        penpot_team_id TEXT NOT NULL,
        name TEXT NOT NULL,
        page_id TEXT,
        last_synced_at INTEGER,
        archived_at INTEGER,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        UNIQUE(project_id, connection_id, penpot_file_id),
        FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
        FOREIGN KEY(connection_id) REFERENCES penpot_connections(id) ON DELETE RESTRICT
    )",
    "CREATE INDEX IF NOT EXISTS idx_penpot_designs_project_active
        ON penpot_designs(project_id, archived_at, updated_at DESC)",
    "CREATE TABLE IF NOT EXISTS penpot_design_conversations (
        id TEXT PRIMARY KEY,
        design_id TEXT NOT NULL,
        agent_id TEXT NOT NULL UNIQUE,
        ordinal INTEGER NOT NULL,
        title TEXT NOT NULL,
        provider TEXT NOT NULL,
        model TEXT NOT NULL,
        external_model_id TEXT,
        external_model_label TEXT,
        external_model_variants TEXT NOT NULL DEFAULT '[]',
        effort TEXT NOT NULL,
        access_mode TEXT NOT NULL,
        chat_session_id TEXT,
        cli_session_id TEXT,
        last_transcript_path TEXT,
        is_current INTEGER NOT NULL DEFAULT 0,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        deleted_at INTEGER,
        UNIQUE(design_id, ordinal),
        FOREIGN KEY(design_id) REFERENCES penpot_designs(id) ON DELETE CASCADE
    )",
    "CREATE UNIQUE INDEX IF NOT EXISTS idx_penpot_conversations_one_current
        ON penpot_design_conversations(design_id) WHERE is_current = 1 AND deleted_at IS NULL",
    "CREATE INDEX IF NOT EXISTS idx_penpot_conversations_design_order
        ON penpot_design_conversations(design_id, deleted_at, ordinal DESC)",
    "CREATE TRIGGER IF NOT EXISTS trg_penpot_conversation_delete_chat
        AFTER DELETE ON penpot_design_conversations
        BEGIN
            DELETE FROM chat_messages WHERE agent_id = OLD.agent_id;
            DELETE FROM chat_timeline_events WHERE agent_id = OLD.agent_id;
            DELETE FROM attachments WHERE agent_id = OLD.agent_id;
            DELETE FROM agent_runtime_sessions WHERE agent_id = OLD.agent_id;
        END",
];

const SCHEMA_V16: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS memories (
        id TEXT PRIMARY KEY,
        scope TEXT NOT NULL,
        project_id TEXT,
        text TEXT NOT NULL,
        enabled INTEGER NOT NULL DEFAULT 1,
        pinned INTEGER NOT NULL DEFAULT 0,
        source_agent_id TEXT,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        last_used_at INTEGER,
        FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
    )",
    "CREATE INDEX IF NOT EXISTS idx_memories_scope_project
        ON memories(scope, project_id, updated_at DESC)",
];

const SCHEMA_V26: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS project_git_workflows (
        id TEXT PRIMARY KEY,
        project_id TEXT NOT NULL,
        repository_path TEXT NOT NULL,
        name TEXT NOT NULL COLLATE NOCASE,
        source TEXT NOT NULL,
        destination TEXT NOT NULL,
        completion_policy TEXT NOT NULL,
        sort_order INTEGER NOT NULL,
        FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
    )",
    "CREATE UNIQUE INDEX IF NOT EXISTS idx_project_git_workflows_unique_name
        ON project_git_workflows(project_id, repository_path, name COLLATE NOCASE)",
    "CREATE INDEX IF NOT EXISTS idx_project_git_workflows_project_order
        ON project_git_workflows(project_id, repository_path, sort_order)",
    "CREATE TABLE IF NOT EXISTS project_git_workflow_runs (
        id TEXT PRIMARY KEY,
        project_id TEXT NOT NULL,
        workflow_id TEXT,
        repository_path TEXT NOT NULL,
        source TEXT NOT NULL,
        destination TEXT NOT NULL,
        pull_request_number INTEGER,
        expected_head_sha TEXT,
        state TEXT NOT NULL,
        error TEXT,
        started_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
        FOREIGN KEY(workflow_id) REFERENCES project_git_workflows(id) ON DELETE SET NULL
    )",
    "CREATE INDEX IF NOT EXISTS idx_project_git_workflow_runs_project_updated
        ON project_git_workflow_runs(project_id, repository_path, updated_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_project_git_workflow_runs_workflow
        ON project_git_workflow_runs(workflow_id, updated_at DESC)",
];

const SCHEMA_V27: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS agent_summaries (
        agent_id TEXT PRIMARY KEY,
        summary_text TEXT NOT NULL,
        outcome_text TEXT,
        last_summarized_sequence INTEGER NOT NULL DEFAULT -1,
        updated_at INTEGER NOT NULL,
        edited_by_user INTEGER NOT NULL DEFAULT 0,
        FOREIGN KEY(agent_id) REFERENCES agents(id) ON DELETE CASCADE
    )",
    "CREATE INDEX IF NOT EXISTS idx_agent_summaries_updated
        ON agent_summaries(updated_at DESC)",
    "CREATE TABLE IF NOT EXISTS agent_search_fts (
        agent_id TEXT PRIMARY KEY,
        project_id TEXT NOT NULL,
        title TEXT NOT NULL,
        status TEXT NOT NULL,
        summary_text TEXT NOT NULL,
        updated_at INTEGER NOT NULL
    )",
    "CREATE INDEX IF NOT EXISTS idx_agent_search_project
        ON agent_search_fts(project_id, updated_at DESC)",
    // Turso's embedded engine exposes its FTS5-equivalent index through
    // `USING fts`; it does not support SQLite `CREATE VIRTUAL TABLE ... fts5`
    // syntax. This remains a real inverted full-text index, not a LIKE scan.
    "CREATE INDEX IF NOT EXISTS idx_agent_search_fts_text
        ON agent_search_fts USING fts (title, summary_text)",
    "CREATE TABLE IF NOT EXISTS chat_messages_fts (
        message_id TEXT PRIMARY KEY,
        agent_id TEXT NOT NULL,
        text TEXT NOT NULL
    )",
    "CREATE INDEX IF NOT EXISTS idx_chat_messages_fts_text
        ON chat_messages_fts USING fts (text)",
    "CREATE TABLE IF NOT EXISTS agent_messages (
        id TEXT PRIMARY KEY,
        source_agent_id TEXT NOT NULL,
        target_agent_id TEXT NOT NULL,
        text TEXT NOT NULL,
        kind TEXT NOT NULL DEFAULT 'agent',
        event_key TEXT,
        created_at INTEGER NOT NULL,
        delivered_at INTEGER,
        FOREIGN KEY(source_agent_id) REFERENCES agents(id) ON DELETE CASCADE,
        FOREIGN KEY(target_agent_id) REFERENCES agents(id) ON DELETE CASCADE
    )",
    "CREATE INDEX IF NOT EXISTS idx_agent_messages_target_delivery
        ON agent_messages(target_agent_id, delivered_at, created_at)",
    "CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_messages_target_event_key
        ON agent_messages(target_agent_id, event_key)
        WHERE event_key IS NOT NULL",
];

const SCHEMA_V30: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS chat_file_ledgers (
        agent_id TEXT PRIMARY KEY,
        revision INTEGER NOT NULL DEFAULT 0,
        updated_at INTEGER NOT NULL,
        FOREIGN KEY(agent_id) REFERENCES agents(id) ON DELETE CASCADE
    )",
    "CREATE TABLE IF NOT EXISTS chat_file_ledger (
        agent_id TEXT NOT NULL,
        path TEXT NOT NULL,
        attribution TEXT NOT NULL,
        additions INTEGER NOT NULL,
        deletions INTEGER NOT NULL,
        baseline_hash TEXT,
        result_hash TEXT,
        baseline_content TEXT,
        result_content TEXT,
        updated_at INTEGER NOT NULL,
        PRIMARY KEY(agent_id, path),
        FOREIGN KEY(agent_id) REFERENCES agents(id) ON DELETE CASCADE
    )",
    "CREATE INDEX IF NOT EXISTS idx_chat_file_ledger_agent_attribution_path
        ON chat_file_ledger(agent_id, attribution, path)",
];

const SCHEMA_V32: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS pending_project_script_presets (
        id TEXT PRIMARY KEY,
        project_id TEXT NOT NULL,
        name TEXT NOT NULL COLLATE NOCASE,
        command TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        UNIQUE(project_id, name),
        FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
    )",
    "CREATE INDEX IF NOT EXISTS idx_pending_project_script_presets_project_created
        ON pending_project_script_presets(project_id, created_at ASC)",
];

const SCHEMA_V33: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS orbit_modules (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL COLLATE NOCASE,
        description TEXT NOT NULL,
        view_type TEXT NOT NULL,
        section_key TEXT,
        section_label TEXT,
        agent_job TEXT NOT NULL,
        revision INTEGER NOT NULL DEFAULT 1,
        archived INTEGER NOT NULL DEFAULT 0,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    )",
    "CREATE UNIQUE INDEX IF NOT EXISTS idx_orbit_modules_active_name
        ON orbit_modules(name COLLATE NOCASE) WHERE archived = 0",
    "CREATE TABLE IF NOT EXISTS orbit_module_fields (
        id TEXT PRIMARY KEY,
        module_id TEXT NOT NULL,
        field_key TEXT NOT NULL,
        label TEXT NOT NULL,
        kind TEXT NOT NULL,
        is_primary INTEGER NOT NULL DEFAULT 0,
        sort_order INTEGER NOT NULL,
        archived INTEGER NOT NULL DEFAULT 0,
        UNIQUE(module_id, field_key),
        FOREIGN KEY(module_id) REFERENCES orbit_modules(id) ON DELETE CASCADE
    )",
    "CREATE INDEX IF NOT EXISTS idx_orbit_module_fields_order
        ON orbit_module_fields(module_id, archived, sort_order)",
    "CREATE TABLE IF NOT EXISTS orbit_project_modules (
        project_id TEXT NOT NULL,
        module_key TEXT NOT NULL,
        enabled INTEGER NOT NULL DEFAULT 1,
        sort_order INTEGER NOT NULL,
        data_revision INTEGER NOT NULL DEFAULT 0,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        PRIMARY KEY(project_id, module_key),
        FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
    )",
    "CREATE INDEX IF NOT EXISTS idx_orbit_project_modules_order
        ON orbit_project_modules(project_id, enabled, sort_order)",
    "CREATE TABLE IF NOT EXISTS orbit_records (
        id TEXT PRIMARY KEY,
        project_id TEXT NOT NULL,
        module_id TEXT NOT NULL,
        section_value TEXT,
        values_json TEXT NOT NULL,
        record_key TEXT NOT NULL,
        source_agent_id TEXT,
        source_batch_id TEXT,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        UNIQUE(project_id, module_id, record_key),
        FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
        FOREIGN KEY(module_id) REFERENCES orbit_modules(id) ON DELETE CASCADE,
        FOREIGN KEY(source_agent_id) REFERENCES agents(id) ON DELETE SET NULL
    )",
    "CREATE INDEX IF NOT EXISTS idx_orbit_records_project_module
        ON orbit_records(project_id, module_id, section_value, updated_at)",
    "CREATE TABLE IF NOT EXISTS orbit_invocations (
        id TEXT PRIMARY KEY,
        agent_id TEXT NOT NULL,
        project_id TEXT NOT NULL,
        module_id TEXT NOT NULL,
        module_revision INTEGER NOT NULL,
        data_revision INTEGER NOT NULL,
        expires_at INTEGER NOT NULL,
        completed_at INTEGER,
        created_at INTEGER NOT NULL,
        FOREIGN KEY(agent_id) REFERENCES agents(id) ON DELETE CASCADE,
        FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
        FOREIGN KEY(module_id) REFERENCES orbit_modules(id) ON DELETE CASCADE
    )",
    "CREATE INDEX IF NOT EXISTS idx_orbit_invocations_agent_created
        ON orbit_invocations(agent_id, created_at DESC)",
    "CREATE TABLE IF NOT EXISTS orbit_mutation_batches (
        id TEXT PRIMARY KEY,
        invocation_id TEXT NOT NULL,
        agent_id TEXT NOT NULL,
        project_id TEXT NOT NULL,
        module_id TEXT NOT NULL,
        revision_before INTEGER NOT NULL,
        revision_after INTEGER NOT NULL,
        before_json TEXT NOT NULL,
        after_json TEXT NOT NULL,
        inserted_count INTEGER NOT NULL,
        updated_count INTEGER NOT NULL,
        deleted_count INTEGER NOT NULL,
        created_at INTEGER NOT NULL,
        undone_at INTEGER,
        FOREIGN KEY(invocation_id) REFERENCES orbit_invocations(id) ON DELETE CASCADE,
        FOREIGN KEY(agent_id) REFERENCES agents(id) ON DELETE CASCADE,
        FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
        FOREIGN KEY(module_id) REFERENCES orbit_modules(id) ON DELETE CASCADE
    )",
    "CREATE INDEX IF NOT EXISTS idx_orbit_batches_invocation_created
        ON orbit_mutation_batches(invocation_id, created_at ASC)",
];

const SCHEMA_V34: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS quick_ask_exchanges (
        id TEXT PRIMARY KEY,
        session_id TEXT NOT NULL,
        project_id TEXT,
        project_name TEXT,
        question TEXT NOT NULL,
        answer TEXT NOT NULL,
        provider TEXT NOT NULL,
        model_label TEXT NOT NULL,
        created_at INTEGER NOT NULL
    )",
    "CREATE INDEX IF NOT EXISTS idx_quick_ask_exchanges_created
        ON quick_ask_exchanges(created_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_quick_ask_exchanges_session
        ON quick_ask_exchanges(session_id, created_at)",
];

pub(super) async fn schema_version(conn: &Connection) -> Result<u32> {
    let mut rows = conn
        .query(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            (),
        )
        .await?;
    let Some(row) = rows.next().await? else {
        return Ok(0);
    };
    let value: i64 = row.get(0)?;
    Ok(value.max(0) as u32)
}

pub(super) async fn record_schema_version(conn: &Connection, version: u32) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
        (version as i64, u64_to_i64(unix_now())?),
    )
    .await?;
    Ok(())
}

pub(super) async fn column_exists(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    let sql = format!("PRAGMA table_info({table})");
    let mut rows = conn.query(sql, ()).await?;
    while let Some(row) = rows.next().await? {
        let name: String = row.get(1)?;
        if name == column {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) async fn table_exists(conn: &Connection, table: &str) -> Result<bool> {
    let mut rows = conn
        .query(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1 LIMIT 1",
            [table],
        )
        .await?;
    Ok(rows.next().await?.is_some())
}

const SCHEMA_V1: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS project_sections (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        collapsed INTEGER NOT NULL DEFAULT 0
    )",
    "CREATE TABLE IF NOT EXISTS projects (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        path TEXT NOT NULL,
        icon TEXT NOT NULL,
        icon_color TEXT NOT NULL,
        icon_image_path TEXT,
        section_id TEXT,
        is_favorite INTEGER NOT NULL DEFAULT 0,
        FOREIGN KEY(section_id) REFERENCES project_sections(id) ON DELETE SET NULL
    )",
    "CREATE TABLE IF NOT EXISTS project_presets (
        id TEXT PRIMARY KEY,
        project_id TEXT NOT NULL,
        name TEXT NOT NULL,
        command TEXT NOT NULL,
        sort_order INTEGER NOT NULL,
        FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
    )",
    "CREATE TABLE IF NOT EXISTS project_db_connections (
        id TEXT PRIMARY KEY,
        project_id TEXT NOT NULL,
        provider TEXT NOT NULL DEFAULT 'mongodb',
        read_only INTEGER NOT NULL DEFAULT 0,
        name TEXT NOT NULL,
        uri TEXT NOT NULL,
        sort_order INTEGER NOT NULL,
        FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
    )",
    "CREATE TABLE IF NOT EXISTS agents (
        id TEXT PRIMARY KEY,
        project_id TEXT NOT NULL,
        project_path TEXT NOT NULL,
        repository_path TEXT,
        title TEXT NOT NULL,
        doc TEXT NOT NULL,
        notes TEXT NOT NULL,
        status TEXT NOT NULL,
        provider TEXT NOT NULL,
        runtime TEXT NOT NULL,
        model TEXT NOT NULL,
        effort TEXT NOT NULL,
        access_mode TEXT NOT NULL,
        source_doc TEXT,
        hidden_doc_assistant INTEGER NOT NULL DEFAULT 0,
        cli_session_id TEXT,
        chat_session_id TEXT,
        ship_pr_repo_path TEXT,
        ship_pr_branch TEXT,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        started_at INTEGER,
        FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
    )",
    "CREATE TABLE IF NOT EXISTS agent_linked_docs (
        agent_id TEXT NOT NULL,
        path TEXT NOT NULL,
        sort_order INTEGER NOT NULL,
        PRIMARY KEY(agent_id, path),
        FOREIGN KEY(agent_id) REFERENCES agents(id) ON DELETE CASCADE
    )",
    "CREATE TABLE IF NOT EXISTS agent_changed_files (
        agent_id TEXT NOT NULL,
        path TEXT NOT NULL,
        additions INTEGER NOT NULL,
        deletions INTEGER NOT NULL,
        PRIMARY KEY(agent_id, path),
        FOREIGN KEY(agent_id) REFERENCES agents(id) ON DELETE CASCADE
    )",
    "CREATE TABLE IF NOT EXISTS agent_runtime_sessions (
        agent_id TEXT NOT NULL,
        kind TEXT NOT NULL,
        session_id TEXT NOT NULL,
        updated_at INTEGER NOT NULL,
        PRIMARY KEY(agent_id, kind, session_id)
    )",
    "CREATE TABLE IF NOT EXISTS chat_messages (
        id TEXT PRIMARY KEY,
        agent_id TEXT NOT NULL,
        role TEXT NOT NULL,
        text TEXT NOT NULL,
        sequence INTEGER NOT NULL,
        created_at INTEGER NOT NULL,
        backend_message_id TEXT
    )",
    "CREATE TABLE IF NOT EXISTS chat_timeline_events (
        id TEXT PRIMARY KEY,
        agent_id TEXT NOT NULL,
        kind TEXT NOT NULL,
        payload_json TEXT NOT NULL,
        sequence INTEGER NOT NULL,
        created_at INTEGER NOT NULL
    )",
    "CREATE TABLE IF NOT EXISTS attachments (
        id TEXT PRIMARY KEY,
        agent_id TEXT NOT NULL,
        message_id TEXT,
        original_name TEXT NOT NULL,
        mime_type TEXT,
        size_bytes INTEGER NOT NULL,
        sha256 TEXT NOT NULL,
        relative_path TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        state TEXT NOT NULL
    )",
    "CREATE INDEX IF NOT EXISTS idx_agents_project_status_updated
        ON agents(project_id, status, updated_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_agents_project_updated
        ON agents(project_id, updated_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_chat_messages_agent_sequence
        ON chat_messages(agent_id, sequence)",
    "CREATE INDEX IF NOT EXISTS idx_attachments_agent_message
        ON attachments(agent_id, message_id)",
    "CREATE INDEX IF NOT EXISTS idx_agent_changed_files_agent_path
        ON agent_changed_files(agent_id, path)",
];

const SCHEMA_V2: &[&str] = &[
    "CREATE INDEX IF NOT EXISTS idx_chat_timeline_events_agent_sequence
        ON chat_timeline_events(agent_id, sequence)",
    "CREATE UNIQUE INDEX IF NOT EXISTS idx_chat_timeline_events_agent_kind_key
        ON chat_timeline_events(agent_id, kind, event_key)
        WHERE event_key IS NOT NULL",
];

const SCHEMA_V3: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS agent_diff_snapshots (
        id TEXT PRIMARY KEY,
        agent_id TEXT NOT NULL,
        project_id TEXT NOT NULL,
        repo_path TEXT NOT NULL,
        source TEXT NOT NULL,
        base_sha TEXT,
        head_sha TEXT,
        commit_sha TEXT,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        state TEXT NOT NULL
    )",
    "CREATE TABLE IF NOT EXISTS agent_diff_files (
        snapshot_id TEXT NOT NULL,
        path TEXT NOT NULL,
        additions INTEGER NOT NULL,
        deletions INTEGER NOT NULL,
        is_binary INTEGER NOT NULL DEFAULT 0,
        diff_json TEXT NOT NULL,
        truncated INTEGER NOT NULL DEFAULT 0,
        sort_order INTEGER NOT NULL,
        PRIMARY KEY(snapshot_id, path),
        FOREIGN KEY(snapshot_id) REFERENCES agent_diff_snapshots(id) ON DELETE CASCADE
    )",
    "CREATE INDEX IF NOT EXISTS idx_agent_diff_snapshots_agent_created
        ON agent_diff_snapshots(agent_id, created_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_agent_diff_snapshots_commit
        ON agent_diff_snapshots(commit_sha)",
    "CREATE INDEX IF NOT EXISTS idx_agent_diff_files_snapshot_order
        ON agent_diff_files(snapshot_id, sort_order)",
];

const SCHEMA_V4: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS project_references (
        id TEXT PRIMARY KEY,
        project_id TEXT NOT NULL,
        kind TEXT NOT NULL,
        title TEXT NOT NULL,
        source TEXT NOT NULL,
        preview_relative_path TEXT,
        notes TEXT NOT NULL,
        metadata_json TEXT NOT NULL,
        sort_order INTEGER NOT NULL,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
    )",
    "CREATE INDEX IF NOT EXISTS idx_project_references_project_order
        ON project_references(project_id, sort_order, created_at)",
];

const SCHEMA_V5: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS project_task_tracker_connections (
        id TEXT PRIMARY KEY,
        project_id TEXT NOT NULL,
        provider TEXT NOT NULL,
        name TEXT NOT NULL,
        site_url TEXT NOT NULL,
        email TEXT NOT NULL,
        api_token TEXT NOT NULL,
        board_id INTEGER,
        board_name TEXT,
        assignee_filter TEXT,
        assignee_account_id TEXT,
        assignee_display_name TEXT,
        sort_order INTEGER NOT NULL,
        FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
    )",
    "CREATE TABLE IF NOT EXISTS agent_linked_tasks (
        agent_id TEXT NOT NULL,
        provider TEXT NOT NULL,
        site_url TEXT NOT NULL,
        issue_id TEXT NOT NULL,
        issue_key TEXT NOT NULL,
        issue_url TEXT NOT NULL,
        title TEXT NOT NULL,
        is_source INTEGER NOT NULL DEFAULT 0,
        sort_order INTEGER NOT NULL,
        PRIMARY KEY(agent_id, provider, site_url, issue_key),
        FOREIGN KEY(agent_id) REFERENCES agents(id) ON DELETE CASCADE
    )",
    "CREATE INDEX IF NOT EXISTS idx_project_task_trackers_project_order
        ON project_task_tracker_connections(project_id, sort_order)",
    "CREATE INDEX IF NOT EXISTS idx_agent_linked_tasks_agent_order
        ON agent_linked_tasks(agent_id, sort_order)",
];

const SCHEMA_V8: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS personal_tasks (
        id TEXT PRIMARY KEY,
        project_id TEXT NOT NULL,
        key_number INTEGER NOT NULL,
        title TEXT NOT NULL,
        description_markdown TEXT NOT NULL,
        status TEXT NOT NULL,
        priority TEXT NOT NULL,
        labels_json TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        archived INTEGER NOT NULL DEFAULT 0,
        UNIQUE(project_id, key_number),
        FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
    )",
    "CREATE INDEX IF NOT EXISTS idx_personal_tasks_project_status
        ON personal_tasks(project_id, archived, status, key_number)",
];

const SCHEMA_V9: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS personal_task_comments (
        id TEXT PRIMARY KEY,
        task_id TEXT NOT NULL,
        author TEXT NOT NULL,
        body TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        FOREIGN KEY(task_id) REFERENCES personal_tasks(id) ON DELETE CASCADE
    )",
    "CREATE INDEX IF NOT EXISTS idx_personal_task_comments_task
        ON personal_task_comments(task_id, created_at)",
];

const SCHEMA_V13: &[&str] = &[];

const SCHEMA_V14: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS project_previews (
        id TEXT PRIMARY KEY,
        project_id TEXT NOT NULL,
        url TEXT NOT NULL,
        title TEXT NOT NULL DEFAULT '',
        source_agent_id TEXT,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        UNIQUE(project_id, url),
        FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
        FOREIGN KEY(source_agent_id) REFERENCES agents(id) ON DELETE SET NULL
    )",
    "CREATE INDEX IF NOT EXISTS idx_project_previews_project_updated
        ON project_previews(project_id, updated_at DESC)",
];

type TxFuture<'a> = std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + 'a>>;

pub(super) async fn execute_transaction<'a>(
    conn: &'a Connection,
    f: impl FnOnce(&'a Connection) -> TxFuture<'a>,
) -> Result<()> {
    // Every caller performs writes. Reserve the writer before the closure does
    // any reads so another connection cannot commit between our read snapshot
    // and our first write, which Turso rejects with BusySnapshot.
    conn.execute("BEGIN IMMEDIATE", ()).await?;
    let result = f(conn).await;
    match result {
        Ok(()) => {
            if let Err(error) = conn.execute("COMMIT", ()).await {
                let _ = conn.execute("ROLLBACK", ()).await;
                return Err(error.into());
            }
            Ok(())
        }
        Err(error) => {
            let _ = conn.execute("ROLLBACK", ()).await;
            Err(error)
        }
    }
}

pub(super) async fn get_meta(conn: &Connection, key: &str) -> Result<Option<String>> {
    let mut rows = conn
        .query("SELECT value FROM meta WHERE key = ?1", [key])
        .await?;
    let Some(row) = rows.next().await? else {
        return Ok(None);
    };
    Ok(Some(row.get(0)?))
}

pub(super) async fn set_meta(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)",
        (key, value),
    )
    .await?;
    Ok(())
}

pub(super) async fn table_count(conn: &Connection, table: &str) -> Result<i64> {
    let sql = format!("SELECT COUNT(*) FROM {table}");
    let mut rows = conn.query(sql, ()).await?;
    let Some(row) = rows.next().await? else {
        return Ok(0);
    };
    Ok(row.get(0)?)
}
