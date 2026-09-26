use ide_core::local_store::LocalStore;
use std::path::Path;
use turso::{Builder, Connection};

async fn connect(path: &Path) -> Connection {
    Builder::new_local(path.to_str().unwrap())
        .experimental_index_method(true)
        .build()
        .await
        .unwrap()
        .connect()
        .unwrap()
}

async fn schema_cookie(conn: &Connection) -> i64 {
    conn.query("PRAGMA schema_version", ())
        .await
        .unwrap()
        .next()
        .await
        .unwrap()
        .unwrap()
        .get(0)
        .unwrap()
}

async fn assert_searchable_rows(conn: &Connection, chat: &str) {
    let mut rows = conn
        .query(
            "SELECT agent_id, summary_text FROM agent_search_fts
             WHERE fts_match(title, summary_text, 'Compatibility')",
            (),
        )
        .await
        .unwrap();
    let row = rows.next().await.unwrap().unwrap();
    assert_eq!(row.get::<String>(0).unwrap(), "agent-one");
    assert_eq!(row.get::<String>(1).unwrap(), "Retained agent summary");
    assert!(rows.next().await.unwrap().is_none());
    let mut rows = conn
        .query(
            "SELECT text FROM chat_messages_fts WHERE fts_match(text, 'transcript')",
            (),
        )
        .await
        .unwrap();
    assert_eq!(
        rows.next()
            .await
            .unwrap()
            .unwrap()
            .get::<String>(0)
            .unwrap(),
        chat
    );
    assert!(rows.next().await.unwrap().is_none());
}

#[test]
fn upgrades_072_fts_indexes_preserves_rows_and_does_not_rebuild_on_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.db");
    std::fs::write(&path, include_bytes!("fixtures/turso-0.7.2-fts.db")).unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();

    // Prove this fixture contains the incompatible format, not an index
    // created by the Turso version under test.
    rt.block_on(async {
        let conn = connect(&path).await;
        let mut rows = conn
            .query(
                "SELECT text FROM chat_messages_fts WHERE fts_match(text, 'transcript')",
                (),
            )
            .await
            .unwrap();
        let error = rows.next().await.unwrap_err();
        assert!(error.to_string().contains("created by an older version"));
    });

    drop(LocalStore::open(dir.path().to_path_buf()).unwrap());
    let cookie = rt.block_on(async {
        let conn = connect(&path).await;
        assert_searchable_rows(&conn, "Retained chat transcript").await;
        conn.execute(
            "UPDATE chat_messages_fts SET text = 'Updated chat transcript' WHERE message_id = 'message-one'",
            (),
        )
        .await
        .unwrap();
        schema_cookie(&conn).await
    });

    drop(LocalStore::open(dir.path().to_path_buf()).unwrap());
    rt.block_on(async {
        let conn = connect(&path).await;
        assert_searchable_rows(&conn, "Updated chat transcript").await;
        assert_eq!(schema_cookie(&conn).await, cookie, "reopen rebuilt indexes");
        // Also exercise an existing database at the current application
        // schema version: engine-format migration must not be skipped.
        conn.execute(
            "DELETE FROM meta WHERE key = 'turso_fts_storage_format'",
            (),
        )
        .await
        .unwrap();
    });

    drop(LocalStore::open(dir.path().to_path_buf()).unwrap());
    rt.block_on(async {
        let conn = connect(&path).await;
        assert_searchable_rows(&conn, "Updated chat transcript").await;
        assert!(schema_cookie(&conn).await > cookie);
    });
}

#[test]
fn failed_fts_rebuild_rolls_back_and_can_be_retried() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.db");
    std::fs::write(&path, include_bytes!("fixtures/turso-0.7.2-fts.db")).unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let conn = connect(&path).await;
        conn.execute("DROP INDEX idx_chat_messages_fts_text", ())
            .await
            .unwrap();
        conn.execute(
            "ALTER TABLE chat_messages_fts RENAME COLUMN text TO legacy_text",
            (),
        )
        .await
        .unwrap();
    });

    // Rebuilding the second index fails after the first has been rebuilt.
    assert!(LocalStore::open(dir.path().to_path_buf()).is_err());
    rt.block_on(async {
        let conn = connect(&path).await;
        let mut rows = conn
            .query(
                "SELECT title FROM agent_search_fts WHERE fts_match(title, summary_text, 'Compatibility')",
                (),
            )
            .await
            .unwrap();
        assert!(rows.next().await.unwrap_err().to_string().contains("created by an older version"));
        drop(rows);
        let mut rows = conn
            .query("SELECT value FROM meta WHERE key = 'turso_fts_storage_format'", ())
            .await
            .unwrap();
        assert!(rows.next().await.unwrap().is_none());
        drop(rows);
        conn.execute("ALTER TABLE chat_messages_fts RENAME COLUMN legacy_text TO text", ())
            .await
            .unwrap();
    });

    drop(LocalStore::open(dir.path().to_path_buf()).unwrap());
    rt.block_on(async {
        assert_searchable_rows(&connect(&path).await, "Retained chat transcript").await;
    });
}
