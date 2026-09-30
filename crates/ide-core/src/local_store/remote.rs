use super::*;

pub(super) async fn ensure_schema(conn: &Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS remote_command_receipts (
        device_id TEXT NOT NULL, command_id TEXT NOT NULL, fingerprint TEXT NOT NULL,
        response TEXT, created_at INTEGER NOT NULL, PRIMARY KEY(device_id, command_id)
    )",
        (),
    )
    .await?;
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
pub enum RemoteReceipt {
    New,
    Complete(String),
    Pending,
    Conflict,
}

impl LocalStore {
    /// Reserve before execution. An interrupted reservation is never executed
    /// twice: clients must refresh/reconcile it instead of inventing a new ID.
    pub fn reserve_remote_command(
        &self,
        device: &str,
        command: &str,
        fingerprint: &str,
    ) -> Result<RemoteReceipt> {
        anyhow::ensure!(
            !command.is_empty() && command.len() <= 128,
            "Invalid command identity"
        );
        self.rt.block_on(async {
            let conn = self.connect().await?;
            let inserted = conn.execute("INSERT OR IGNORE INTO remote_command_receipts (device_id,command_id,fingerprint,created_at) VALUES (?1,?2,?3,?4)", params![device, command, fingerprint, unix_now() as i64]).await?;
            if inserted == 1 { return Ok(RemoteReceipt::New); }
            let mut rows = conn.query("SELECT fingerprint, response FROM remote_command_receipts WHERE device_id=?1 AND command_id=?2", params![device, command]).await?;
            let row = rows.next().await?.ok_or_else(||anyhow!("Command reservation disappeared"))?;
            if row.get::<String>(0)? != fingerprint { return Ok(RemoteReceipt::Conflict); }
            Ok(match opt_text(&row, 1)? { Some(response) => RemoteReceipt::Complete(response), None => RemoteReceipt::Pending })
        })
    }

    pub fn complete_remote_command(
        &self,
        device: &str,
        command: &str,
        response: &str,
    ) -> Result<()> {
        self.rt.block_on(async {
            self.connect().await?.execute("UPDATE remote_command_receipts SET response=?3 WHERE device_id=?1 AND command_id=?2 AND response IS NULL", params![device, command, response]).await?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remote_receipts_survive_reopen_and_are_device_and_payload_scoped() {
        let root = std::env::temp_dir().join(format!("choro-remote-receipts-{}", Uuid::new_v4()));
        let store = LocalStore::open(root.clone()).unwrap();
        assert_eq!(
            store
                .reserve_remote_command("phone", "one", "payload")
                .unwrap(),
            RemoteReceipt::New
        );
        assert_eq!(
            store
                .reserve_remote_command("phone", "one", "payload")
                .unwrap(),
            RemoteReceipt::Pending
        );
        assert_eq!(
            store
                .reserve_remote_command("phone", "one", "other")
                .unwrap(),
            RemoteReceipt::Conflict
        );
        store
            .complete_remote_command("phone", "one", "result")
            .unwrap();
        drop(store);
        let reopened = LocalStore::open(root).unwrap();
        assert_eq!(
            reopened
                .reserve_remote_command("phone", "one", "payload")
                .unwrap(),
            RemoteReceipt::Complete("result".into())
        );
        assert_eq!(
            reopened
                .reserve_remote_command("other-phone", "one", "payload")
                .unwrap(),
            RemoteReceipt::New
        );
    }
}
