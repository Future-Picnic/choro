use super::*;
use crate::agent_changes::{ChangeReceipt, MutationEvidence, WorkspaceChangeSnapshot};

pub(super) async fn ensure_schema(conn: &Connection) -> Result<()> {
    for sql in [
        "CREATE TABLE IF NOT EXISTS mutation_evidence (sequence INTEGER PRIMARY KEY AUTOINCREMENT, event_key TEXT NOT NULL UNIQUE, project_id TEXT NOT NULL, agent_id TEXT NOT NULL, root TEXT NOT NULL, path TEXT NOT NULL, captured_at INTEGER NOT NULL, confirmed INTEGER NOT NULL, payload TEXT NOT NULL, metadata TEXT NOT NULL DEFAULT '{}')",
        "CREATE INDEX IF NOT EXISTS mutation_evidence_scope ON mutation_evidence(project_id,root,sequence)",
        "CREATE TABLE IF NOT EXISTS change_receipts (agent_id TEXT NOT NULL, generation TEXT NOT NULL, turn_id TEXT NOT NULL, payload TEXT NOT NULL, PRIMARY KEY(agent_id,generation,turn_id))",
        "CREATE TABLE IF NOT EXISTS workspace_change_snapshots (root TEXT PRIMARY KEY, payload TEXT NOT NULL)",
        "CREATE TABLE IF NOT EXISTS change_tracking_health (id INTEGER PRIMARY KEY, incomplete INTEGER NOT NULL)",
        "CREATE TABLE IF NOT EXISTS change_settlements (root TEXT NOT NULL, path TEXT NOT NULL, through_time INTEGER NOT NULL, PRIMARY KEY(root,path))",
    ] { conn.execute(sql, ()).await?; }
    if !super::schema::column_exists(conn, "mutation_evidence", "metadata").await? {
        conn.execute(
            "ALTER TABLE mutation_evidence ADD COLUMN metadata TEXT NOT NULL DEFAULT '{}'",
            (),
        )
        .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_changes::*;

    fn evidence(
        project: Uuid,
        agent: Uuid,
        root: &Path,
        action: &str,
        time: u64,
    ) -> MutationEvidence {
        MutationEvidence {
            key: ChangeKey {
                project_id: project,
                agent_id: agent,
                root: root.to_path_buf(),
                generation: "generation".into(),
                turn_id: "turn".into(),
                action_id: action.into(),
                path: "shared.rs".into(),
            },
            kind: EvidenceKind::Contents,
            confirmed: true,
            additions: Some(1),
            deletions: Some(1),
            before_hash: None,
            after_hash: None,
            before: Some("before\n".into()),
            after: Some("after\n".into()),
            patch: None,
            captured_at: time,
        }
    }

    #[test]
    fn version_37_upgrade_adds_receipt_health_without_losing_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalStore::open(dir.path().join("data")).unwrap();
        let project = Uuid::new_v4();
        let agent = Uuid::new_v4();
        store
            .save_mutation_evidence(&evidence(project, agent, dir.path(), "action", 1))
            .unwrap();
        store.rt.block_on(async {
            let conn = store.connect().await.unwrap();
            conn.execute("DROP TABLE change_tracking_health", ())
                .await
                .unwrap();
            super::super::schema::record_schema_version(&conn, 37)
                .await
                .unwrap();
        });
        drop(store);
        let store = LocalStore::open(dir.path().join("data")).unwrap();
        store.mark_change_tracking_overflow().unwrap();
        let result = store
            .query_agent_changes(project, agent, dir.path(), &[], 0)
            .unwrap();
        assert_eq!(result["tracking_incomplete"], true);
        assert_eq!(result["own_edits"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn evidence_is_immutable_scoped_and_settles_for_both_writers() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalStore::open(dir.path().join("data")).unwrap();
        let project = Uuid::new_v4();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let root = dir.path().join("repo");
        let ea = evidence(project, a, &root, "a", 10);
        let eb = evidence(project, b, &root, "b", 11);
        store.save_mutation_evidence(&ea).unwrap();
        store.save_mutation_evidence(&eb).unwrap();
        let mut duplicate = ea.clone();
        duplicate.after = Some("unrelated later text".into());
        store.save_mutation_evidence(&duplicate).unwrap();
        assert_eq!(
            store
                .load_turn_mutation_evidence(a, "generation", "turn")
                .unwrap()[0]
                .after,
            ea.after
        );
        let query = store
            .query_agent_changes(project, a, &root, &[], 0)
            .unwrap();
        assert_eq!(query["own_edits"].as_array().unwrap().len(), 1);
        assert_eq!(query["other_agent_edits"].as_array().unwrap().len(), 1);
        for actor in [a, b] {
            assert!(store
                .pending_agent_repository_paths(actor, &root)
                .unwrap()
                .contains(Path::new("shared.rs")));
            assert!(store
                .pending_agent_repository_paths(actor, &root.join("separate-worktree"))
                .unwrap()
                .is_empty());
        }
        assert!(store
            .query_agent_changes(Uuid::new_v4(), a, &root, &[], 0)
            .unwrap()["own_edits"]
            .as_array()
            .unwrap()
            .is_empty());
        assert!(store
            .query_agent_changes(project, a, &root.join("other-lane"), &[], 0)
            .unwrap()["own_edits"]
            .as_array()
            .unwrap()
            .is_empty());
        store
            .settle_change_paths(&root, &["shared.rs".into()], 12)
            .unwrap();
        for agent in [a, b] {
            assert!(store
                .pending_agent_repository_paths(agent, &root)
                .unwrap()
                .is_empty());
            assert!(store
                .pending_agent_change_paths(agent, &root)
                .unwrap()
                .is_empty());
        }
        // Evidence queued before the commit is settled even when persisted late.
        store
            .save_mutation_evidence(&evidence(project, b, &root, "late-before-commit", 11))
            .unwrap();
        assert!(store
            .pending_agent_change_paths(b, &root)
            .unwrap()
            .is_empty());
        store
            .save_mutation_evidence(&evidence(project, b, &root, "after-commit", 13))
            .unwrap();
        assert!(store
            .pending_agent_change_paths(a, &root)
            .unwrap()
            .is_empty());
        assert!(store
            .pending_agent_change_paths(b, &root)
            .unwrap()
            .contains(Path::new("shared.rs")));
        let reopened = LocalStore::open(dir.path().join("data")).unwrap();
        assert_eq!(
            reopened
                .load_turn_mutation_evidence(a, "generation", "turn")
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn observations_and_oversized_payloads_do_not_claim_exact_edits() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalStore::open(dir.path().join("data")).unwrap();
        let project = Uuid::new_v4();
        let agent = Uuid::new_v4();
        let root = dir.path();
        let mut e = evidence(project, agent, root, "shell", 10);
        e.confirmed = false;
        e.kind = EvidenceKind::Observation;
        e.after = Some("a".repeat(MAX_CONTENT_BYTES + 1));
        store.save_mutation_evidence(&e).unwrap();
        let q = store
            .query_agent_changes(project, agent, root, &[], 0)
            .unwrap();
        assert!(q["own_edits"].as_array().unwrap().is_empty());
        assert!(q["unattributed_evidence"][0]["additions"].is_null());
        assert!(store
            .pending_agent_change_paths(agent, root)
            .unwrap()
            .is_empty());
        store
            .save_workspace_change_snapshot(&WorkspaceChangeSnapshot {
                root: root.to_path_buf(),
                paths: (0..205)
                    .map(|i| PathBuf::from(format!("file{i}")))
                    .collect(),
                complete: false,
                error: Some("watcher overflow".into()),
                ..Default::default()
            })
            .unwrap();
        let q = store
            .query_agent_changes(project, agent, root, &[], 0)
            .unwrap();
        assert_eq!(q["workspace"]["paths"].as_array().unwrap().len(), 100);
        assert_eq!(q["next_cursor"], "100");
        assert_eq!(
            store
                .query_agent_changes(project, agent, root, &[], 200)
                .unwrap()["workspace"]["paths"]
                .as_array()
                .unwrap()
                .len(),
            5
        );
    }
}

impl LocalStore {
    /// A constant-size durable backstop if even receipt metadata overflows.
    /// Historical uncertainty remains visible after restart, without inventing
    /// which missing actions took place.
    pub fn mark_change_tracking_overflow(&self) -> Result<()> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            conn.execute(
                "INSERT OR REPLACE INTO change_tracking_health (id,incomplete) VALUES (1,1)",
                (),
            )
            .await?;
            Ok(())
        })
    }

    pub fn pending_workspace_change_paths(&self, root: &Path) -> Result<Vec<PathBuf>> {
        self.rt.block_on(async {
            let conn=self.connect().await?;
            let mut rows=conn.query("SELECT DISTINCT e.path FROM mutation_evidence e LEFT JOIN change_settlements s ON s.root=e.root AND s.path=e.path WHERE e.root=?1 AND e.confirmed=1 AND e.captured_at>COALESCE(s.through_time,0)",[path_to_string(root)]).await?;
            let mut paths=Vec::new();while let Some(row)=rows.next().await? {paths.push(PathBuf::from(row.get::<String>(0)?));}Ok(paths)
        })
    }

    pub fn change_scope_roots(&self) -> Result<Vec<PathBuf>> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            let mut rows = conn
                .query("SELECT DISTINCT root FROM mutation_evidence", ())
                .await?;
            let mut roots = Vec::new();
            while let Some(row) = rows.next().await? {
                roots.push(PathBuf::from(row.get::<String>(0)?));
            }
            Ok(roots)
        })
    }

    pub fn load_turn_mutation_evidence(
        &self,
        agent: Uuid,
        generation: &str,
        turn: &str,
    ) -> Result<Vec<MutationEvidence>> {
        self.rt.block_on(async {
            let conn=self.connect().await?;
            let mut rows=conn.query("SELECT payload FROM mutation_evidence WHERE agent_id=?1 AND json_extract(payload,'$.key.generation')=?2 AND json_extract(payload,'$.key.turn_id')=?3 ORDER BY sequence LIMIT 4096",params![agent.to_string(),generation,turn]).await?;
            let mut result=Vec::new();
            let mut bytes=0usize;
            while let Some(row)=rows.next().await? {let raw=row.get::<String>(0)?;bytes=bytes.saturating_add(raw.len());
                if bytes>crate::agent_changes::MAX_PENDING_BYTES {break;}
                result.push(serde_json::from_str(&raw)?);}
            Ok(result)
        })
    }

    pub fn save_mutation_evidence(&self, evidence: &MutationEvidence) -> Result<()> {
        self.insert_mutation_evidence(evidence).map(|_| ())
    }

    /// Returns false for a notification already persisted, even after a turn
    /// was sealed or the router dropped its in-memory state.
    pub fn insert_mutation_evidence(&self, evidence: &MutationEvidence) -> Result<bool> {
        let mut evidence = evidence.clone();
        evidence.bound();
        self.rt.block_on(async {
            let conn = self.connect().await?;
            let changed = conn.execute("INSERT OR IGNORE INTO mutation_evidence (event_key,project_id,agent_id,root,path,captured_at,confirmed,payload,metadata) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)", params![evidence.id(), evidence.key.project_id.to_string(), evidence.key.agent_id.to_string(), path_to_string(&evidence.key.root), path_to_string(&evidence.key.path), evidence.captured_at as i64, i64::from(evidence.confirmed), serde_json::to_string(&evidence)?, serde_json::json!({"path":evidence.key.path,"agent_id":evidence.key.agent_id,"turn_id":evidence.key.turn_id,"action_id":evidence.key.action_id,"evidence":evidence.kind,"confirmed":evidence.confirmed,"additions":evidence.additions,"deletions":evidence.deletions,"captured_at":evidence.captured_at}).to_string()]).await?;
            Ok(changed > 0)
        })
    }

    pub fn load_change_receipt(
        &self,
        agent: Uuid,
        generation: &str,
        turn: &str,
    ) -> Result<Option<ChangeReceipt>> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            let mut rows = conn.query("SELECT payload FROM change_receipts WHERE agent_id=?1 AND generation=?2 AND turn_id=?3", params![agent.to_string(), generation, turn]).await?;
            rows.next().await?.map(|row| Ok(serde_json::from_str(&row.get::<String>(0)?)?)).transpose()
        })
    }

    pub fn turn_mutation_count(&self, agent: Uuid, generation: &str, turn: &str) -> Result<usize> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            let mut rows = conn.query("SELECT COUNT(*) FROM mutation_evidence WHERE agent_id=?1 AND json_extract(payload,'$.key.generation')=?2 AND json_extract(payload,'$.key.turn_id')=?3",params![agent.to_string(),generation,turn]).await?;
            Ok(rows.next().await?.unwrap().get::<i64>(0)? as usize)
        })
    }

    pub fn save_change_receipt(&self, receipt: &ChangeReceipt) -> Result<()> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            let sql=if receipt.state==crate::agent_changes::ChangeReceiptState::Pending {"INSERT OR IGNORE INTO change_receipts (agent_id,generation,turn_id,payload) VALUES (?1,?2,?3,?4)"} else {"INSERT OR REPLACE INTO change_receipts (agent_id,generation,turn_id,payload) VALUES (?1,?2,?3,?4)"};
            conn.execute(sql, params![receipt.agent_id.to_string(), receipt.generation.clone(), receipt.turn_id.clone(), serde_json::to_string(receipt)?]).await?;
            Ok(())
        })
    }

    pub fn save_workspace_change_snapshot(&self, snapshot: &WorkspaceChangeSnapshot) -> Result<()> {
        let mut snapshot = snapshot.clone();
        snapshot.root = crate::agent_changes::working_directory(&snapshot.root);
        self.rt.block_on(async {
            let conn = self.connect().await?;
            conn.execute(
                "INSERT OR REPLACE INTO workspace_change_snapshots (root,payload) VALUES (?1,?2)",
                params![
                    path_to_string(&snapshot.root),
                    serde_json::to_string(&snapshot)?
                ],
            )
            .await?;
            Ok(())
        })
    }

    pub fn load_workspace_change_snapshot(
        &self,
        root: &Path,
    ) -> Result<Option<WorkspaceChangeSnapshot>> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            let mut rows = conn
                .query(
                    "SELECT payload FROM workspace_change_snapshots WHERE root=?1",
                    [path_to_string(&crate::agent_changes::working_directory(
                        root,
                    ))],
                )
                .await?;
            rows.next()
                .await?
                .map(|r| Ok(serde_json::from_str(&r.get::<String>(0)?)?))
                .transpose()
        })
    }

    /// A clean boundary settles only evidence captured before the status read.
    /// This also covers evidence still queued when a different agent ships.
    pub fn settle_change_paths(
        &self,
        root: &Path,
        paths: &[PathBuf],
        through_time: u64,
    ) -> Result<()> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            for path in paths {
                conn.execute("INSERT INTO change_settlements (root,path,through_time) VALUES (?1,?2,?3) ON CONFLICT(root,path) DO UPDATE SET through_time=MAX(through_time,excluded.through_time)", params![path_to_string(root), path_to_string(path), through_time as i64]).await?;
            }
            Ok(())
        })
    }

    pub fn pending_agent_change_paths(
        &self,
        agent_id: Uuid,
        root: &Path,
    ) -> Result<HashSet<PathBuf>> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            let mut rows = conn.query("SELECT DISTINCT e.path FROM mutation_evidence e LEFT JOIN change_settlements s ON s.root=e.root AND s.path=e.path WHERE e.agent_id=?1 AND e.root=?2 AND e.confirmed=1 AND e.captured_at>COALESCE(s.through_time,0)", params![agent_id.to_string(),path_to_string(root)]).await?;
            let mut result = HashSet::new();
            while let Some(row) = rows.next().await? { result.insert(PathBuf::from(row.get::<String>(0)?)); }
            Ok(result)
        })
    }

    /// Read inside the repository Ship lock so another dialog's settlement is
    /// reflected even before its UI refresh arrives. Nested scopes are mapped
    /// to repository-relative paths without reading file contents.
    pub fn pending_agent_repository_paths(
        &self,
        agent_id: Uuid,
        repository: &Path,
    ) -> Result<HashSet<PathBuf>> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            let mut rows = conn.query("SELECT DISTINCT e.root,e.path FROM mutation_evidence e LEFT JOIN change_settlements s ON s.root=e.root AND s.path=e.path WHERE e.agent_id=?1 AND e.confirmed=1 AND e.captured_at>COALESCE(s.through_time,0)", [agent_id.to_string()]).await?;
            let mut result = HashSet::new();
            while let Some(row) = rows.next().await? {
                let absolute = PathBuf::from(row.get::<String>(0)?).join(row.get::<String>(1)?);
                if let Some(path) = crate::agent_changes::relative_path(repository, &absolute) {
                    result.insert(path);
                }
            }
            Ok(result)
        })
    }

    /// Bounded, persisted-only query. No git commands or filesystem scans.
    pub fn query_agent_changes(
        &self,
        project_id: Uuid,
        agent_id: Uuid,
        root: &Path,
        paths: &[PathBuf],
        cursor: i64,
    ) -> Result<serde_json::Value> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            let mut sql = "SELECT e.sequence,e.metadata,e.captured_at>COALESCE(s.through_time,0) FROM mutation_evidence e LEFT JOIN change_settlements s ON s.root=e.root AND s.path=e.path WHERE e.project_id=?1 AND e.root=?2".to_string();
            let mut values = vec![Value::Text(project_id.to_string()), Value::Text(path_to_string(root)), Value::Integer(cursor)];
            if !paths.is_empty() {
                sql.push_str(" AND e.path IN (");
                for (i,path) in paths.iter().enumerate() { if i>0 {sql.push(',');} sql.push_str(&format!("?{}",i+4)); values.push(Value::Text(path_to_string(path))); }
                sql.push(')');
            }
            sql.push_str(" ORDER BY e.sequence LIMIT 101 OFFSET ?3");
            let mut rows = conn.query(&sql, values).await?;
            let mut own = Vec::new(); let mut other = Vec::new(); let mut unknown = Vec::new(); let mut count=0;
            while let Some(row) = rows.next().await? {
                count += 1; if count > 100 { break; }
                let mut item:serde_json::Value=serde_json::from_str(&row.get::<String>(1)?)?;
                item["pending"]=serde_json::json!(row.get::<i64>(2)? != 0);
                if item["confirmed"]!=true {unknown.push(item);} else if item["agent_id"].as_str()==Some(&agent_id.to_string()) {own.push(item);} else {other.push(item);}

            }
            drop(rows);
            let mut rows = conn.query("SELECT payload FROM change_receipts WHERE agent_id=?1 ORDER BY rowid DESC LIMIT 20", [agent_id.to_string()]).await?;
            let mut receipts = Vec::new();
            while let Some(row) = rows.next().await? { receipts.push(serde_json::from_str::<ChangeReceipt>(&row.get::<String>(0)?)?); }
            drop(rows);
            let mut rows = conn.query("SELECT payload FROM workspace_change_snapshots WHERE root=?1", [path_to_string(&crate::agent_changes::working_directory(root))]).await?;
            let snapshot = rows.next().await?.map(|r| -> Result<WorkspaceChangeSnapshot> { Ok(serde_json::from_str(&r.get::<String>(0)?)?) }).transpose()?;
            let mut workspace_more = false;
            let snapshot=snapshot.map(|mut s| {
                s.paths.retain(|p| paths.is_empty() || paths.contains(p));
                workspace_more = s.paths.len() > (cursor as usize).saturating_add(100);
                s.paths=s.paths.into_iter().skip(cursor as usize).take(100).collect();
                s.statuses.retain(|p,_| s.paths.contains(p));s
            });
            let mut health = conn.query("SELECT incomplete FROM change_tracking_health WHERE id=1", ()).await?;
            let tracking_incomplete = health.next().await?.is_some_and(|row| row.get::<i64>(0).unwrap_or(1) != 0);
            Ok(serde_json::json!({"tracking_incomplete":tracking_incomplete,"own_edits":own,"other_agent_edits":other,"unattributed_evidence":unknown,"workspace":snapshot,"receipts":receipts,"next_cursor":if count>100 || workspace_more {Some(cursor.saturating_add(100).to_string())} else {None},"note":"Workspace paths can include your edits and unrelated edits in the same file. Evidence is historical, not exclusive ownership. Pending/partial receipts and stale observations are not proof of a clean workspace."}))
        })
    }
}
