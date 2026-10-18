use super::*;
use crate::ship_actions::*;

pub(super) async fn ensure_schema(conn: &Connection) -> Result<()> {
    for sql in [
        "CREATE TABLE IF NOT EXISTS ship_actions_watches (id TEXT PRIMARY KEY, project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE, host TEXT NOT NULL, repository TEXT NOT NULL, pr_number INTEGER NOT NULL, revision INTEGER NOT NULL, payload TEXT NOT NULL, UNIQUE(project_id,host,repository,pr_number))",
        "CREATE TABLE IF NOT EXISTS ship_actions_origins (watch_id TEXT NOT NULL REFERENCES ship_actions_watches(id) ON DELETE CASCADE, agent_id TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE, ship_id TEXT NOT NULL, payload TEXT NOT NULL, PRIMARY KEY(watch_id,agent_id,ship_id))",
        "CREATE INDEX IF NOT EXISTS ship_actions_origin_agent ON ship_actions_origins(agent_id)",
        "CREATE TABLE IF NOT EXISTS ship_actions_runs (watch_id TEXT NOT NULL REFERENCES ship_actions_watches(id) ON DELETE CASCADE, run_id INTEGER NOT NULL, payload TEXT NOT NULL, PRIMARY KEY(watch_id,run_id))",
    ] { conn.execute(sql, ()).await?; }
    Ok(())
}

pub(super) async fn save_watch(conn: &Connection, watch: &ShipActionsWatch) -> Result<()> {
    let mut rows = conn
        .query(
            "SELECT revision FROM ship_actions_watches WHERE id=?1",
            [watch.id.to_string()],
        )
        .await?;
    if let Some(row) = rows.next().await? {
        if i64_to_u64(row.get(0)?)? >= watch.revision {
            return Ok(());
        }
    }
    drop(rows);
    let mut payload = watch.clone();
    payload.origins.clear();
    payload.runs.clear();
    conn.execute("INSERT INTO ship_actions_watches(id,project_id,host,repository,pr_number,revision,payload) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(id) DO UPDATE SET revision=excluded.revision,payload=excluded.payload",
        params![watch.id.to_string(),watch.project_id.0.to_string(),watch.host.clone(),watch.repository.clone(),u64_to_i64(watch.pr_number)?,u64_to_i64(watch.revision)?,serde_json::to_string(&payload)?]).await?;
    for origin in &watch.origins {
        conn.execute("INSERT INTO ship_actions_origins(watch_id,agent_id,ship_id,payload) VALUES(?1,?2,?3,?4) ON CONFLICT(watch_id,agent_id,ship_id) DO UPDATE SET payload=excluded.payload",
            params![watch.id.to_string(),origin.agent_id.to_string(),origin.ship_id.clone(),serde_json::to_string(origin)?]).await?;
    }
    for run in &watch.runs {
        conn.execute("INSERT INTO ship_actions_runs(watch_id,run_id,payload) VALUES(?1,?2,?3) ON CONFLICT(watch_id,run_id) DO UPDATE SET payload=excluded.payload",
            params![watch.id.to_string(),u64_to_i64(run.id)?,serde_json::to_string(run)?]).await?;
    }
    Ok(())
}

pub(super) async fn load_watches(conn: &Connection) -> Result<Vec<ShipActionsWatch>> {
    let mut rows = conn
        .query(
            "SELECT payload FROM ship_actions_watches ORDER BY rowid",
            (),
        )
        .await?;
    let mut watches = vec![];
    while let Some(row) = rows.next().await? {
        watches.push(serde_json::from_str::<ShipActionsWatch>(
            &row.get::<String>(0)?,
        )?);
    }
    drop(rows);
    for watch in &mut watches {
        let mut rows = conn
            .query(
                "SELECT payload FROM ship_actions_origins WHERE watch_id=?1 ORDER BY rowid",
                [watch.id.to_string()],
            )
            .await?;
        while let Some(row) = rows.next().await? {
            watch
                .origins
                .push(serde_json::from_str(&row.get::<String>(0)?)?);
        }
        drop(rows);
        let mut rows = conn
            .query(
                "SELECT payload FROM ship_actions_runs WHERE watch_id=?1 ORDER BY run_id",
                [watch.id.to_string()],
            )
            .await?;
        while let Some(row) = rows.next().await? {
            watch
                .runs
                .push(serde_json::from_str(&row.get::<String>(0)?)?);
        }
    }
    Ok(watches)
}

impl LocalStore {
    pub fn save_ship_actions_watch(&self, watch: &ShipActionsWatch) -> Result<()> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            execute_transaction(&conn, |conn| {
                Box::pin(async move { save_watch(conn, watch).await })
            })
            .await
        })
    }
    pub fn load_ship_actions_watches(&self) -> Result<Vec<ShipActionsWatch>> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            load_watches(&conn).await
        })
    }
}
