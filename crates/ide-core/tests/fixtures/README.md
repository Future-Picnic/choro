# Turso storage compatibility fixture

`turso-0.7.2-fts.db` is a synthetic database generated with the official
`turso = "=0.7.2"` Rust crate, with default features disabled and `fts` enabled.
It contains no user data. Its old FTS storage must be unreadable by Turso 0.8
until Choro rebuilds the indexes; the integration test verifies this first.

To regenerate, open a new database with
`Builder::new_local(path).experimental_index_method(true)` using Turso 0.7.2,
execute the SQL below, and fully consume the final checkpoint query before
closing the connection. The checkpoint leaves a self-contained database with
an empty WAL. Only the `.db` belongs in this fixture.

```sql
CREATE TABLE agent_search_fts (
    agent_id TEXT PRIMARY KEY, project_id TEXT NOT NULL, title TEXT NOT NULL,
    status TEXT NOT NULL, summary_text TEXT NOT NULL, updated_at INTEGER NOT NULL
);
CREATE INDEX idx_agent_search_fts_text
    ON agent_search_fts USING fts(title, summary_text);
CREATE TABLE chat_messages_fts (
    message_id TEXT PRIMARY KEY, agent_id TEXT NOT NULL, text TEXT NOT NULL
);
CREATE INDEX idx_chat_messages_fts_text ON chat_messages_fts USING fts(text);
INSERT INTO agent_search_fts VALUES (
    'agent-one', 'project-one', 'Compatibility report', 'idle',
    'Retained agent summary', 123
);
INSERT INTO chat_messages_fts VALUES (
    'message-one', 'agent-one', 'Retained chat transcript'
);
PRAGMA wal_checkpoint(TRUNCATE);
```
