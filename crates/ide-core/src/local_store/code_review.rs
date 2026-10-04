//! One reserved writer serializes reviewer reports, cancellation and shutdown.
use super::*;
use crate::code_review::*;
use anyhow::ensure;
use std::io::Write;

async fn scope_revision(conn: &Connection, parent: Uuid) -> Result<u64> {
    let mut rows = conn.query("SELECT (SELECT COALESCE(MAX(sequence),0) FROM mutation_evidence WHERE agent_id=?1) + (SELECT COALESCE(MAX(revision),0) FROM chat_file_ledgers WHERE agent_id=?1)", [parent.to_string()]).await?;
    Ok(rows.next().await?.context("Review scope revision unavailable")?.get::<i64>(0)? as u64)
}

pub(super) async fn ensure_schema(conn: &Connection) -> Result<()> {
    for sql in [
        "CREATE TABLE IF NOT EXISTS code_review_runs (id TEXT PRIMARY KEY, parent_id TEXT NOT NULL, active INTEGER NOT NULL, revision INTEGER NOT NULL, payload TEXT NOT NULL)",
        "CREATE UNIQUE INDEX IF NOT EXISTS review_one_active_parent ON code_review_runs(parent_id) WHERE active = 1",
    ] { conn.execute(sql, ()).await?; }
    Ok(())
}
async fn read_run(conn: &Connection, id: Uuid) -> Result<ReviewRun> {
    let mut rows = conn
        .query(
            "SELECT payload FROM code_review_runs WHERE id=?1",
            [id.to_string()],
        )
        .await?;
    let row = rows.next().await?.context("Review run is unavailable")?;
    let run: ReviewRun = serde_json::from_str(&row.get::<String>(0)?)?;
    ensure!(
        run.id == id && run.version == REVIEW_VERSION,
        "Stored review identity or version is invalid"
    );
    Ok(run)
}
async fn write_run(conn: &Connection, run: &ReviewRun) -> Result<()> {
    conn.execute("INSERT INTO code_review_runs(id,parent_id,active,revision,payload) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(id) DO UPDATE SET active=excluded.active,revision=excluded.revision,payload=excluded.payload",
        params![run.id.to_string(),run.parent_id.to_string(),i64::from(!run.state.terminal()),run.revision as i64,serde_json::to_string(run)?]).await?;
    Ok(())
}
impl LocalStore {
    pub fn review_storage(&self, id: Uuid) -> PathBuf {
        self.app_data_dir().join("code-review").join(id.to_string())
    }
    pub fn save_review_input(&self, run: &ReviewRun, input: &ReviewInput) -> Result<()> {
        validate_input(run, input)?;
        let dir = self.review_storage(run.id);
        fs::create_dir_all(&dir)?;
        let bytes = serde_json::to_vec(input)?;
        ensure!(
            bytes.len() <= MAX_REVIEW_MANIFEST_BYTES,
            "Review metadata exceeds 32 MiB; preparation stopped without dropping scope"
        );
        ensure!(serde_json::to_vec(&input.requirements)?.len() <= MAX_REVIEW_CONTEXT_BYTES,"Frozen conversation context exceeds 4 MiB; preparation stopped without dropping requirements");
        let path = dir.join("input.json");
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        Ok(())
    }
    pub fn load_review_input(&self, run: &ReviewRun) -> Result<ReviewInput> {
        let path = self.review_storage(run.id).join("input.json");
        let meta = fs::symlink_metadata(&path)?;
        ensure!(
            meta.is_file()
                && !meta.file_type().is_symlink()
                && meta.len() <= MAX_REVIEW_MANIFEST_BYTES as u64,
            "Invalid review manifest"
        );
        let input = serde_json::from_slice(&fs::read(path)?)?;
        validate_input(run, &input)?;
        Ok(input)
    }
    pub fn create_review_run(&self, run: &ReviewRun) -> Result<()> {
        ensure!(
            run.version == REVIEW_VERSION
                && run.state == ReviewRunState::Preparing
                && run.revision == 0,
            "New review must be preparing"
        );
        self.rt.block_on(async {
            let conn = self.connect().await?;
            execute_transaction(&conn, |conn| Box::pin(async move {
                // INSERT, rather than upsert, protects against duplicated starts.
                conn.execute("INSERT INTO code_review_runs(id,parent_id,active,revision,payload) VALUES(?1,?2,1,0,?3)",
                    params![run.id.to_string(),run.parent_id.to_string(),serde_json::to_string(run)?]).await?;
                Ok(())
            })).await
        })
    }
    pub fn load_review_run(&self, id: Uuid) -> Result<ReviewRun> {
        self.rt
            .block_on(async { read_run(&self.connect().await?, id).await })
    }
    pub fn load_review_runs(&self, parent: Uuid) -> Result<Vec<ReviewRun>> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            let mut rows = conn
                .query(
                    "SELECT payload FROM code_review_runs WHERE parent_id=?1 ORDER BY rowid",
                    [parent.to_string()],
                )
                .await?;
            let mut runs = vec![];
            while let Some(row) = rows.next().await? {
                runs.push(serde_json::from_str(&row.get::<String>(0)?)?);
            }
            Ok(runs)
        })
    }
    /// A cancellation and a late report cannot both win. Callers must validate
    /// their role/run before touching the aggregate; closures roll back on error.
    pub fn transact_review<T>(
        &self,
        id: Uuid,
        expected_revision: Option<u64>,
        operation: impl FnOnce(&mut ReviewRun) -> Result<T>,
    ) -> Result<(ReviewRun, T)> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            conn.execute("BEGIN IMMEDIATE", ()).await?;
            let result = async {
                let mut run = read_run(&conn, id).await?;
                ensure!(
                    expected_revision.is_none_or(|r| r == run.revision),
                    "Review changed; reload before retrying"
                );
                let previous = run.clone();
                let output = operation(&mut run)?;
                ensure!(
                    run.id == previous.id
                        && run.parent_id == previous.parent_id
                        && run.reviewer_id == previous.reviewer_id
                        && run.snapshot_id == previous.snapshot_id,
                    "Immutable review identity changed"
                );
                run.revision = previous.revision + 1;
                write_run(&conn, &run).await?;
                Ok((run, output))
            }
            .await;
            match result {
                Ok(value) => {
                    if let Err(e) = conn.execute("COMMIT", ()).await {
                        let _ = conn.execute("ROLLBACK", ()).await;
                        return Err(e.into());
                    }
                    Ok(value)
                }
                Err(e) => {
                    let _ = conn.execute("ROLLBACK", ()).await;
                    Err(e)
                }
            }
        })
    }
    pub fn interrupt_unfinished_reviews(&self, parent: Uuid) -> Result<Vec<ReviewRun>> {
        let mut results = vec![];
        for run in self
            .load_review_runs(parent)?
            .into_iter()
            .filter(|r| !r.state.terminal())
        {
            results.push(
                self.transact_review(run.id, None, |run| {
                    run.stop(
                        ReviewRunState::Interrupted,
                        Some(
                            "Choro restarted before this review finished; coverage is incomplete"
                                .into(),
                        ),
                        review_now(),
                    );
                    Ok(())
                })?
                .0,
            );
        }
        Ok(results)
    }
    pub fn interrupt_all_unfinished_reviews(&self) -> Result<Vec<ReviewRun>> {
        let ids = self.rt.block_on(async {
            let conn = self.connect().await?;
            let mut rows = conn
                .query("SELECT id FROM code_review_runs WHERE active=1", ())
                .await?;
            let mut ids = vec![];
            while let Some(row) = rows.next().await? {
                ids.push(Uuid::parse_str(&row.get::<String>(0)?)?);
            }
            Ok::<_, anyhow::Error>(ids)
        })?;
        let mut runs = vec![];
        for id in ids {
            runs.push(
                self.transact_review(id, None, |run| {
                    run.stop(
                        ReviewRunState::Interrupted,
                        Some(
                            "Choro restarted before this review finished; coverage is incomplete"
                                .into(),
                        ),
                        review_now(),
                    );
                    Ok(())
                })?
                .0,
            );
        }
        Ok(runs)
    }
    pub fn review_mutation_revision(&self, parent: Uuid) -> Result<u64> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            scope_revision(&conn, parent).await
        })
    }
    /// This queries mutation receipts, never Git dirtiness. Attributable commits
    /// stay in scope because their original immutable receipts remain recorded.
    pub fn review_ownership(
        &self,
        parent: Uuid,
    ) -> Result<(
        Vec<crate::agent_changes::MutationEvidence>,
        Vec<String>,
        u64,
    )> {
        use crate::agent_changes::{ChangeReceipt, ChangeReceiptState, MutationEvidence};
        self.rt.block_on(async {
            let conn = self.connect().await?;
            let mut rows = conn.query("SELECT sequence,payload FROM mutation_evidence WHERE agent_id=?1 ORDER BY sequence LIMIT 8193", [parent.to_string()]).await?;
            let revision = scope_revision(&conn, parent).await?;
            let mut evidence: Vec<MutationEvidence> = vec![]; let mut limitations = vec![]; let mut bytes = 0;
            while let Some(row) = rows.next().await? {
                let raw = row.get::<String>(1)?; bytes += raw.len();
                if evidence.len() == 8192 || bytes > MAX_REVIEW_BYTES { limitations.push("Conversation mutation evidence exceeds the review budget; ownership scope is incomplete".into()); break; }
                evidence.push(serde_json::from_str(&raw)?);
            }
            drop(rows);
            let turns: std::collections::BTreeSet<_> = evidence.iter().map(|e| (e.key.generation.clone(),e.key.turn_id.clone())).collect();
            for (generation,turn) in turns {
                let mut rows = conn.query("SELECT payload FROM change_receipts WHERE agent_id=?1 AND generation=?2 AND turn_id=?3", params![parent.to_string(),generation,turn.clone()]).await?;
                let receipt: Option<ChangeReceipt> = rows.next().await?.map(|row| serde_json::from_str(&row.get::<String>(0)?).map_err(anyhow::Error::from)).transpose()?;
                if receipt.is_none_or(|r| r.state != ChangeReceiptState::Ready) { limitations.push(format!("Turn {turn} has incomplete mutation receipts")); }
            }
            let mut health = conn.query("SELECT incomplete FROM change_tracking_health LIMIT 1", ()).await?;
            if health.next().await?.is_some_and(|row| row.get::<i64>(0).unwrap_or(1) != 0) { limitations.push("Change tracking recorded missing evidence".into()); }
            if evidence.iter().any(|e| !e.confirmed) { limitations.push("Unconfirmed mutation observations cannot establish ownership".into()); }
            if let Some(ledger) = super::chat::load_chat_file_ledger_async(&conn,parent).await? {
                let owner = super::agents::load_agents_async(&conn).await?.into_iter().find(|agent| agent.id == parent);
                for entry in ledger.entries.into_iter().filter(|entry| !entry.observed) {
                    if let Some(owner) = &owner {
                        for recovered in confirmed_ledger_evidence(&entry, owner.project_id.0, owner.runtime_path(), &evidence) {
                            let size = serde_json::to_vec(&recovered)?.len();
                            if evidence.len() >= 8192 || bytes.saturating_add(size) > MAX_REVIEW_BYTES {
                                limitations.push("Confirmed ledger contents exceed the review evidence budget".into()); break;
                            }
                            bytes += size; evidence.push(recovered);
                        }
                    }
                    if !evidence.iter().any(|e| e.confirmed && e.key.path == entry.path) {
                        limitations.push(format!("{} is in the conversation ledger but its mutation evidence is unavailable",entry.path.display()));
                    }
                }
            }
            if scope_revision(&conn, parent).await? != revision {
                limitations.push("Conversation ownership changed during preparation; review the latest changes".into());
            }
            Ok((evidence, limitations, revision))
        })
    }
}
