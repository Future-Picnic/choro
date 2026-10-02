//! Delegation aggregates are transactionally replaced under a reserved writer.
//! The coordinator and MCP share these APIs; only AgentRecords creates chats.
use super::*;
use crate::delegation::{DelegationLimits, DelegationRun, OperationReceipt};
use crate::experts::{named_experts, normalized_expert_name, ExpertProfile};
use anyhow::ensure;

async fn beta_features_async(conn: &Connection) -> Result<crate::config::BetaFeatures> {
    get_meta(conn, "beta_features")
        .await?
        .map(|value| serde_json::from_str(&value).map_err(Into::into))
        .unwrap_or(Ok(crate::config::BetaFeatures::UNSET))
}

pub(super) async fn ensure_delegation_schema_inner(conn: &Connection) -> Result<()> {
    for statement in [
        "CREATE TABLE IF NOT EXISTS expert_profiles (id TEXT PRIMARY KEY, name_key TEXT NOT NULL UNIQUE, revision INTEGER NOT NULL, payload_json TEXT NOT NULL)",
        "CREATE TABLE IF NOT EXISTS delegation_runs (id TEXT PRIMARY KEY, parent_agent_id TEXT NOT NULL, active INTEGER NOT NULL, revision INTEGER NOT NULL, payload_json TEXT NOT NULL, FOREIGN KEY(parent_agent_id) REFERENCES agents(id) ON DELETE RESTRICT)",
        "CREATE UNIQUE INDEX IF NOT EXISTS delegation_one_active_parent ON delegation_runs(parent_agent_id) WHERE active = 1",
        "CREATE TABLE IF NOT EXISTS delegation_authorizations (id TEXT PRIMARY KEY, parent_agent_id TEXT NOT NULL, payload_json TEXT NOT NULL, created_at INTEGER NOT NULL, FOREIGN KEY(parent_agent_id) REFERENCES agents(id) ON DELETE CASCADE)",
    ] { conn.execute(statement, ()).await?; }
    for column in ["expert_json", "delegation_json"] {
        if !column_exists(conn, "agents", column).await? {
            conn.execute(format!("ALTER TABLE agents ADD COLUMN {column} TEXT"), ())
                .await?;
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExpertAuthorization {
    pub id: Uuid,
    pub parent_agent_id: Uuid,
    pub original_assignment: String,
    pub expert_ids: Vec<Uuid>,
    pub plan_mode: bool,
    #[serde(default)]
    pub temporary_experts: Vec<crate::experts::ExpertSnapshot>,
    /// Literal user submissions from a task the user explicitly asked to retry.
    /// Never populated from provider output, repository text or another chat.
    #[serde(default)]
    pub continuation_model_context: Option<String>,
}

fn extend_authorized_team(run: &mut DelegationRun, authorization: &ExpertAuthorization) {
    for id in &authorization.expert_ids {
        if !run.authorized_experts.contains(id) {
            run.authorized_experts.push(*id);
        }
    }
    for expert in &authorization.temporary_experts {
        if !run
            .temporary_experts
            .iter()
            .any(|e| e.profile.id == expert.profile.id)
        {
            run.temporary_model_authorizations.insert(
                expert.profile.id,
                authorization
                    .continuation_model_context
                    .as_ref()
                    .unwrap_or(&authorization.original_assignment)
                    .clone(),
            );
            run.temporary_experts.push(expert.clone());
        }
    }
}

async fn end_transaction<T>(conn: &Connection, result: Result<T>) -> Result<T> {
    match result {
        Ok(value) => match conn.execute("COMMIT", ()).await {
            Ok(_) => Ok(value),
            Err(error) => {
                let _ = conn.execute("ROLLBACK", ()).await;
                Err(error.into())
            }
        },
        Err(error) => {
            let _ = conn.execute("ROLLBACK", ()).await;
            Err(error)
        }
    }
}

pub(super) async fn load_experts_async(conn: &Connection) -> Result<Vec<ExpertProfile>> {
    let mut rows = conn
        .query(
            "SELECT payload_json FROM expert_profiles ORDER BY name_key",
            (),
        )
        .await?;
    let mut profiles = Vec::new();
    while let Some(row) = rows.next().await? {
        profiles.push(serde_json::from_str(&row.get::<String>(0)?)?);
    }
    Ok(profiles)
}

pub(super) async fn load_delegations_async(conn: &Connection) -> Result<Vec<DelegationRun>> {
    let mut rows = conn
        .query(
            "SELECT payload_json FROM delegation_runs ORDER BY rowid",
            (),
        )
        .await?;
    let mut runs = Vec::new();
    while let Some(row) = rows.next().await? {
        runs.push(serde_json::from_str(&row.get::<String>(0)?)?);
    }
    Ok(runs)
}

async fn read_run(conn: &Connection, id: Uuid) -> Result<DelegationRun> {
    let mut rows = conn
        .query(
            "SELECT payload_json FROM delegation_runs WHERE id = ?1",
            [id.to_string()],
        )
        .await?;
    let row = rows
        .next()
        .await?
        .context("Delegation run was not found.")?;
    Ok(serde_json::from_str(&row.get::<String>(0)?)?)
}

pub(super) async fn write_run(conn: &Connection, run: &DelegationRun) -> Result<()> {
    conn.execute("INSERT INTO delegation_runs (id, parent_agent_id, active, revision, payload_json) VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT(id) DO UPDATE SET active = excluded.active, revision = excluded.revision, payload_json = excluded.payload_json", params![run.id.to_string(), run.parent_agent_id.to_string(), if run.status.terminal() { 0i64 } else { 1i64 }, run.revision as i64, serde_json::to_string(run)?]).await?;
    Ok(())
}

impl LocalStore {
    /// Seed shipped profiles once per catalog version. Never replace a user's
    /// edits, disabled/archive state, or an existing profile with the same name.
    pub fn ensure_default_experts(&self) -> Result<()> {
        let catalog = crate::experts::catalog::catalog();
        self.rt.block_on(async {
            let conn = self.connect().await?;
            conn.execute("BEGIN IMMEDIATE", ()).await?;
            let result = async {
                let version = catalog.version.to_string();
                if get_meta(&conn, "expert_catalog_version").await?.as_deref() == Some(version.as_str()) { return Ok(()); }
                for definition in &catalog.experts {
                    let mut profile = definition.profile();
                    profile.validate()?;
                    profile.revision = 1;
                    conn.execute("INSERT OR IGNORE INTO expert_profiles (id, name_key, revision, payload_json) VALUES (?1, ?2, ?3, ?4)", params![profile.id.to_string(), normalized_expert_name(&profile.name), 1i64, serde_json::to_string(&profile)?]).await?;
                }
                set_meta(&conn, "expert_catalog_version", &version).await?;
                Ok(())
            }.await;
            end_transaction(&conn, result).await
        })
    }

    pub fn latest_expert_authorization(&self, parent: Uuid) -> Result<Option<ExpertAuthorization>> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            let mut rows = conn.query("SELECT payload_json FROM delegation_authorizations WHERE parent_agent_id = ?1 ORDER BY created_at DESC, rowid DESC LIMIT 1", [parent.to_string()]).await?;
            rows.next().await?.map(|row| serde_json::from_str(&row.get::<String>(0)?).map_err(Into::into)).transpose()
        })
    }
    pub fn load_experts(&self) -> Result<Vec<ExpertProfile>> {
        self.rt
            .block_on(async { load_experts_async(&self.connect().await?).await })
    }
    pub fn save_expert(
        &self,
        mut profile: ExpertProfile,
        expected_revision: Option<u64>,
    ) -> Result<ExpertProfile> {
        profile.validate()?;
        self.rt.block_on(async {
            let conn = self.connect().await?;
            conn.execute("BEGIN IMMEDIATE", ()).await?;
            let result = async {
                let profiles = load_experts_async(&conn).await?;
                let previous = profiles.iter().find(|p| p.id == profile.id);
                ensure!(previous.map(|p| p.revision) == expected_revision, "This Bandmate changed elsewhere. Reload before saving.");
                let key = normalized_expert_name(&profile.name);
                ensure!(!profiles.iter().any(|p| p.id != profile.id && normalized_expert_name(&p.name) == key), "A Bandmate already uses this name.");
                profile.name = profile.name.split_whitespace().collect::<Vec<_>>().join(" ");
                profile.revision = previous.map_or(1, |p| p.revision + 1);
                conn.execute("INSERT INTO expert_profiles (id, name_key, revision, payload_json) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(id) DO UPDATE SET name_key = excluded.name_key, revision = excluded.revision, payload_json = excluded.payload_json", params![profile.id.to_string(), key, profile.revision as i64, serde_json::to_string(&profile)?]).await?;
                Ok(profile)
            }.await;
            end_transaction(&conn, result).await
        })
    }
    pub fn delegation_limits(&self) -> Result<DelegationLimits> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            get_meta(&conn, "delegation_limits")
                .await?
                .map(|v| serde_json::from_str(&v).map_err(Into::into))
                .unwrap_or_else(|| Ok(DelegationLimits::default()))
        })
    }
    /// Beta opt-ins stay local to this data store; archives do not enable them.
    pub fn beta_features(&self) -> Result<crate::config::BetaFeatures> {
        self.rt
            .block_on(async { beta_features_async(&self.connect().await?).await })
    }
    pub fn save_beta_features(&self, features: crate::config::BetaFeatures) -> Result<()> {
        self.rt.block_on(async {
            set_meta(
                &self.connect().await?,
                "beta_features",
                &serde_json::to_string(&features)?,
            )
            .await
        })
    }
    /// Disabling the beta prevents new runs without stranding an existing team.
    pub fn delegation_available_for(&self, parent: Uuid) -> Result<bool> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            if beta_features_async(&conn).await?.delegation {
                return Ok(true);
            }
            let mut rows = conn.query(
                "SELECT id FROM delegation_runs WHERE parent_agent_id = ?1 AND active = 1 LIMIT 1",
                [parent.to_string()],
            ).await?;
            Ok(rows.next().await?.is_some())
        })
    }
    pub fn save_delegation_limits(&self, limits: &DelegationLimits) -> Result<()> {
        limits.validate()?;
        self.rt.block_on(async {
            set_meta(
                &self.connect().await?,
                "delegation_limits",
                &serde_json::to_string(limits)?,
            )
            .await
        })
    }
    /// Shared desktop/remote submission path. A natural request prepares the
    /// same run as a picker selection, so its authority survives follow-ups.
    /// This does not create tasks or start runtimes; the lead still plans them.
    pub fn prepare_delegation_submission(
        &self,
        parent: Uuid,
        source: Uuid,
        user_text: &str,
        explicit: &[Uuid],
        plan_mode: bool,
    ) -> Result<Option<Uuid>> {
        let runs = self.load_delegations()?;
        let authorization =
            self.authorize_experts(parent, source, user_text, explicit, plan_mode)?;
        if let Some(run) = runs
            .iter()
            .find(|r| r.parent_agent_id == parent && r.status.stopped())
        {
            // Editing the team is allowed while stopped. Scheduling still
            // requires the coordinator's explicit, reconciled Resume path.
            if let Some(authorization) = &authorization {
                self.update_delegation(run.id, Some(run.revision), |r| {
                    extend_authorized_team(r, authorization);
                    Ok(())
                })?;
            }
            return Ok(Some(run.id));
        }
        let has_run = runs
            .iter()
            .any(|r| r.parent_agent_id == parent && !r.status.terminal());
        if !has_run && !self.beta_features()?.delegation {
            ensure!(explicit.is_empty(), "{}", crate::delegation::BETA_DISABLED);
            return Ok(None);
        }
        if !explicit.is_empty() || has_run || crate::experts::requests_delegation(user_text) {
            authorization
                .map(|a| self.begin_delegation(parent, a.id).map(|r| r.id))
                .transpose()
        } else {
            Ok(None)
        }
    }

    /// Trusted application submission path only. MCP does not expose this API.
    pub fn authorize_experts(
        &self,
        parent: Uuid,
        source: Uuid,
        user_text: &str,
        explicit: &[Uuid],
        plan_mode: bool,
    ) -> Result<Option<ExpertAuthorization>> {
        let profiles = self.load_experts()?;
        // Nil is a trusted composer selection, never an MCP-provided profile ID.
        let explicit_temporary = explicit.contains(&Uuid::nil());
        let temporary_requested =
            explicit_temporary || crate::experts::requests_delegation(user_text);
        let explicit = explicit
            .iter()
            .copied()
            .filter(|id| !id.is_nil())
            .collect::<Vec<_>>();
        let mut ids = if explicit_temporary {
            Vec::new()
        } else {
            match named_experts(user_text, &profiles) {
                Ok(ids) => ids,
                // An explicit picker selection resolves an ambiguous textual
                // reference. Do not infer any additional team members in that case.
                Err(_) if !explicit.is_empty() => explicit.to_vec(),
                Err(error) => return Err(error),
            }
        };
        for id in &explicit {
            ensure!(
                profiles
                    .iter()
                    .any(|p| p.id == *id && p.enabled && !p.archived),
                "Selected Bandmate is unavailable."
            );
            if !ids.contains(id) {
                ids.push(*id);
            }
        }
        let mut temporary_experts = Vec::new();
        let on_demand = user_text
            .to_lowercase()
            .replace('-', " ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .contains("on demand");
        if temporary_requested && (ids.is_empty() || explicit_temporary || on_demand) {
            let agents = self.load_agents()?;
            let agent = agents
                .iter()
                .find(|a| a.id == parent)
                .context("Lead chat is unavailable.")?;
            ensure!(
                matches!(
                    agent.provider,
                    crate::AgentKind::Codex | crate::AgentKind::Claude
                ) && agent.runtime == crate::AgentRuntimeKind::Chat
                    && !agent.hidden_doc_assistant
                    && agent.design_context.is_none()
                    && agent
                        .delegation
                        .as_ref()
                        .is_none_or(|b| b.task_id.is_none()),
                "On-demand teammates require an ordinary Codex or Claude lead chat."
            );
            let profile = ExpertProfile {
                id: source, revision: 1, name: "Teammate".into(),
                description: "On-demand teammate for this task".into(),
                provider: agent.provider, model: agent.model.clone(), effort: agent.effort,
                instructions: "Carry out the lead's scoped assignment. Use its brief, relevant context, and expected outcome; coordinate questions through the lead.".into(),
                skills: vec![], expected_outcome: "Return the requested deliverable, evidence, and unresolved items to the lead.".into(),
                enabled: true, archived: false, additions: Default::default(),
            };
            profile.validate()?;
            ids.push(source);
            temporary_experts.push(crate::experts::ExpertSnapshot {
                profile,
                skills: vec![],
            });
        }
        let continuation_model_context = if !temporary_experts.is_empty()
            && explicit.is_empty()
            && !explicit_temporary
            && crate::experts::requests_delegation_continuation(user_text)
        {
            let previous = self
                .load_delegations()?
                .into_iter()
                .rev()
                .find(|r| r.parent_agent_id == parent);
            if let Some(previous) =
                previous.filter(|r| r.status != crate::delegation::RunStatus::Completed)
            {
                let mut context = self.rt.block_on(async {
                    let conn = self.connect().await?;
                    let mut rows = conn.query("SELECT payload_json FROM delegation_authorizations WHERE parent_agent_id = ?1 AND rowid >= (SELECT rowid FROM delegation_authorizations WHERE id = ?2 AND parent_agent_id = ?1) ORDER BY rowid DESC LIMIT 32", (parent.to_string(), previous.source_message_id.to_string())).await?;
                    let mut messages = Vec::new();
                    while let Some(row) = rows.next().await? {
                        let a: ExpertAuthorization = serde_json::from_str(&row.get::<String>(0)?)?;
                        messages.push(a.original_assignment);
                    }
                    messages.reverse();
                    Ok::<_, anyhow::Error>(messages.join("\n\n"))
                })?;
                // Keep the initial brief even when a long conversation exceeds
                // the bounded follow-up window. This context grants models only.
                context = format!(
                    "{}\n\n{context}\n\n{user_text}",
                    previous.original_assignment
                );
                Some(context)
            } else {
                None
            }
        } else {
            None
        };
        let authorization = ExpertAuthorization {
            id: source,
            parent_agent_id: parent,
            original_assignment: user_text.into(),
            expert_ids: ids,
            plan_mode,
            temporary_experts,
            continuation_model_context,
        };
        self.rt.block_on(async {
            let conn = self.connect().await?;
            conn.execute("INSERT INTO delegation_authorizations (id, parent_agent_id, payload_json, created_at) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(id) DO NOTHING", params![source.to_string(), parent.to_string(), serde_json::to_string(&authorization)?, unix_now() as i64]).await?;
            Ok::<(), anyhow::Error>(())
        })?;
        Ok((!authorization.expert_ids.is_empty()).then_some(authorization))
    }
    pub fn begin_delegation(&self, parent: Uuid, authorization_id: Uuid) -> Result<DelegationRun> {
        let limits = self.delegation_limits()?;
        let result = self.rt.block_on(async {
            let conn = self.connect().await?;
            conn.execute("BEGIN IMMEDIATE", ()).await?;
            let result = async {
                let mut rows = conn.query("SELECT payload_json FROM delegation_authorizations WHERE id = ?1 AND parent_agent_id = ?2", (authorization_id.to_string(), parent.to_string())).await?;
                let row = rows.next().await?.context("This assignment was not authorized by a user submission. Ask the user to name the Bandmates.")?;
                let authorization: ExpertAuthorization = serde_json::from_str(&row.get::<String>(0)?)?;
                drop(rows);
                if let Some(previous) = load_delegations_async(&conn).await?.into_iter().find(|r| r.parent_agent_id == parent && r.source_message_id == authorization_id) {
                    return Ok(previous);
                }
                let mut latest=conn.query("SELECT id FROM delegation_authorizations WHERE parent_agent_id = ?1 ORDER BY created_at DESC,rowid DESC LIMIT 1",[parent.to_string()]).await?;
                ensure!(latest.next().await?.is_some_and(|r|r.get::<String>(0).ok().as_deref()==Some(&authorization_id.to_string())),"This authorization belongs to an older user request. Ask the user to name the Bandmates for the current task.");drop(latest);
                let agents = load_agents_async(&conn).await?;
                let agent = agents.iter().find(|a| a.id == parent).context("Lead chat is unavailable.")?;
                ensure!(agent.delegation.as_ref().is_none_or(|b| b.task_id.is_none()), "Bandmates cannot create nested delegations.");
                ensure!(matches!(agent.provider, crate::AgentKind::Codex | crate::AgentKind::Claude) && agent.runtime == crate::AgentRuntimeKind::Chat && !agent.hidden_doc_assistant && agent.design_context.is_none(), "Delegation is available in ordinary Codex and Claude chats.");
                let mut existing = conn.query("SELECT payload_json FROM delegation_runs WHERE parent_agent_id = ?1 AND active = 1", [parent.to_string()]).await?;
                if let Some(row) = existing.next().await? {
                    let mut run: DelegationRun = serde_json::from_str(&row.get::<String>(0)?)?;
                    drop(existing);
                    ensure!(!run.status.stopped(), "Resume the existing task in Choro first.");
                    if authorization.expert_ids.is_empty() {
                        return Ok(run); // A clarification retains the active task's existing team.
                    }
                    extend_authorized_team(&mut run, &authorization);
                    run.revision += 1;
                    write_run(&conn, &run).await?;
                    return Ok(run);
                }
                drop(existing);
                ensure!(!authorization.expert_ids.is_empty(), "No delegation authority was recorded for this user submission. A clear on-demand request does not require saved profiles or /delegate. Report an authorization problem if the request was already clear; retrying this authorization cannot add Bandmates.");
                ensure!(beta_features_async(&conn).await?.delegation, "{}", crate::delegation::BETA_DISABLED);
                let mut run = DelegationRun::new(parent, agent.project_id, authorization.id, authorization.original_assignment.clone(), authorization.expert_ids.clone(), authorization.plan_mode, limits);
                extend_authorized_team(&mut run, &authorization);
                write_run(&conn, &run).await?;
                Ok(run)
            }.await;
            end_transaction(&conn, result).await
        })?;
        crate::preview_control::notify_delegation_changed(&self.root, result.id);
        Ok(result)
    }
    pub fn load_delegations(&self) -> Result<Vec<DelegationRun>> {
        self.rt
            .block_on(async { load_delegations_async(&self.connect().await?).await })
    }
    pub fn load_delegation(&self, id: Uuid) -> Result<DelegationRun> {
        self.rt
            .block_on(async { read_run(&self.connect().await?, id).await })
    }
    pub fn update_delegation<T>(
        &self,
        id: Uuid,
        expected_revision: Option<u64>,
        edit: impl FnOnce(&mut DelegationRun) -> Result<T>,
    ) -> Result<T> {
        let result = self.rt.block_on(async {
            let conn = self.connect().await?;
            conn.execute("BEGIN IMMEDIATE", ()).await?;
            let result = async {
                let mut run = read_run(&conn, id).await?;
                ensure!(
                    expected_revision.is_none_or(|v| v == run.revision),
                    "Delegation revision changed. Read the latest state and retry."
                );
                let output = edit(&mut run)?;
                run.revision += 1;
                write_run(&conn, &run).await?;
                Ok(output)
            }
            .await;
            end_transaction(&conn, result).await
        })?;
        crate::preview_control::notify_delegation_changed(&self.root, id);
        Ok(result)
    }
    /// Provider telemetry does not revise task authority or invalidate a lead's
    /// pending operation. The transaction still preserves concurrent commands.
    pub fn update_delegation_telemetry(
        &self,
        id: Uuid,
        task: Uuid,
        attempt: Uuid,
        session: Option<String>,
        usage: Option<serde_json::Value>,
    ) -> Result<()> {
        let result = self.rt.block_on(async {
            let conn = self.connect().await?;
            conn.execute("BEGIN IMMEDIATE", ()).await?;
            let result = async {
                let mut run = read_run(&conn, id).await?;
                if let Some(a) = run
                    .task_mut(task)?
                    .attempt_mut()
                    .filter(|a| a.id == attempt)
                {
                    a.session_id = session;
                    a.usage = usage;
                    write_run(&conn, &run).await?;
                }
                Ok(())
            }
            .await;
            end_transaction(&conn, result).await
        })?;
        crate::preview_control::notify_delegation_changed(&self.root, id);
        Ok(result)
    }
    pub fn delegation_operation(
        &self,
        id: Uuid,
        caller: Uuid,
        key: &str,
        revision: u64,
        input: &serde_json::Value,
        edit: impl FnOnce(&mut DelegationRun) -> Result<serde_json::Value>,
    ) -> Result<serde_json::Value> {
        ensure!(
            !key.trim().is_empty() && key.len() <= 160,
            "Provide a stable operation_key of at most 160 bytes."
        );
        let input_hash = format!("{:x}", Sha256::digest(serde_json::to_vec(input)?));
        let result = self.rt.block_on(async {
            let conn = self.connect().await?;
            conn.execute("BEGIN IMMEDIATE", ()).await?;
            let result = async {
                let mut run = read_run(&conn, id).await?;
                run.authorize_caller(caller)?;
                let scoped_key = format!("{caller}:{key}");
                if let Some(receipt) = run.operations.get(&scoped_key) {
                    ensure!(
                        receipt.input_hash == input_hash,
                        "operation_key was already used for different input."
                    );
                    // A duplicate receipt is a read, including concurrent retries.
                    // Advancing the revision here invalidates unrelated work.
                    return Ok(receipt.response.clone());
                }
                ensure!(
                    run.revision == revision,
                    "Delegation revision changed. Read the latest state and retry."
                );
                ensure!(
                    !run.status.stopped() && !run.status.terminal(),
                    "This task is paused or finished. Agent operations cannot resume it."
                );
                let response = edit(&mut run)?;
                run.operations.insert(
                    scoped_key,
                    OperationReceipt {
                        input_hash,
                        response: response.clone(),
                    },
                );
                run.revision += 1;
                write_run(&conn, &run).await?;
                Ok(response)
            }
            .await;
            end_transaction(&conn, result).await
        })?;
        crate::preview_control::notify_delegation_changed(&self.root, id);
        Ok(result)
    }

    pub fn replay_delegation_operation(
        &self,
        id: Uuid,
        caller: Uuid,
        key: &str,
        input: &serde_json::Value,
    ) -> Result<Option<serde_json::Value>> {
        let run = self.load_delegation(id)?;
        run.authorize_caller(caller)?;
        let hash = format!("{:x}", Sha256::digest(serde_json::to_vec(input)?));
        if let Some(receipt) = run.operations.get(&format!("{caller}:{key}")) {
            ensure!(
                receipt.input_hash == hash,
                "operation_key was already used for different input."
            );
            return Ok(Some(receipt.response.clone()));
        }
        Ok(None)
    }
    pub fn claim_delegation_coordinator(&self, owner: Uuid) -> Result<bool> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            conn.execute("BEGIN IMMEDIATE", ()).await?;
            let result = async {
                let now = unix_now();
                let lease = get_meta(&conn, "delegation_coordinator_lease")
                    .await?
                    .and_then(|s| serde_json::from_str::<(Uuid, u64)>(&s).ok());
                if lease.is_some_and(|(current, timestamp)| {
                    current != owner && now.saturating_sub(timestamp) < 15
                }) {
                    return Ok(false);
                }
                set_meta(
                    &conn,
                    "delegation_coordinator_lease",
                    &serde_json::to_string(&(owner, now))?,
                )
                .await?;
                Ok(true)
            }
            .await;
            end_transaction(&conn, result).await
        })
    }
    pub fn delegation_cleanup_preview(&self, id: Uuid) -> Result<Vec<std::path::PathBuf>> {
        let run = self.load_delegation(id)?;
        ensure!(
            run.status.terminal(),
            "Finish or cancel the run before cleaning its working copies."
        );
        let root = self.app_data_dir().join("delegation").join(id.to_string());
        let mut paths = Vec::new();
        for task in &run.tasks {
            for a in &task.attempts {
                ensure!(
                    a.workspace == root.join(format!("attempt-{}", a.id)),
                    "Working-copy identity changed. Cleanup refused."
                );
                if !a.working_copy_cleaned && a.workspace.exists() {
                    ensure!(
                        !std::fs::symlink_metadata(&a.workspace)?
                            .file_type()
                            .is_symlink(),
                        "Cleanup refuses linked working copies."
                    );
                    paths.push(a.workspace.clone());
                }
            }
        }
        Ok(paths)
    }
    /// The UI must first show `delegation_cleanup_preview` and get an explicit
    /// user confirmation. History, immutable blobs, and reports remain intact.
    pub fn cleanup_delegation_workspaces(
        &self,
        id: Uuid,
        confirmed_paths: &[std::path::PathBuf],
    ) -> Result<()> {
        let paths = self.delegation_cleanup_preview(id)?;
        ensure!(
            paths == confirmed_paths,
            "Working copies changed after the cleanup preview. Review them again."
        );
        for path in &paths {
            std::fs::remove_dir_all(path)?;
        }
        self.update_delegation(id, None, |r| {
            for task in &mut r.tasks {
                for a in &mut task.attempts {
                    if paths.contains(&a.workspace) {
                        a.working_copy_cleaned = true;
                    }
                }
            }
            r.event(
                r.parent_agent_id,
                None,
                "cleanup",
                "User confirmed removal of private working copies; reports and snapshots retained",
            );
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::delegation::RunStatus;
    use crate::{
        AgentAccessMode, AgentKind, AgentModel, AgentRecord, AgentRuntimeKind, AppConfig, Project,
    };
    use serde_json::json;

    #[test]
    fn delegation_defaults_on_and_persists_independently_of_workspace_saves() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
        let observer = LocalStore::open(dir.path().to_path_buf()).unwrap();
        assert!(store.beta_features().unwrap().delegation);
        let old: crate::config::BetaFeatures = serde_json::from_str("{}").unwrap();
        assert!(!old.delegation);
        let legacy: crate::config::BetaFeatures = serde_json::from_str(r#"{"delegation":true,"penpot":true}"#).unwrap();
        // A pre-release Penpot opt-in must not survive into the launch build.
        assert!(legacy.delegation);
        assert!(serde_json::to_string(&legacy).unwrap().contains("delegation"));
        assert!(!serde_json::to_string(&legacy).unwrap().contains("penpot"));
        store
            .save_beta_features(crate::config::BetaFeatures { delegation: true, ..Default::default() })
            .unwrap();
        store.save_workspace_config(&AppConfig::default()).unwrap();
        assert!(observer.beta_features().unwrap().delegation);
        assert!(observer.beta_features().unwrap().delegation);
        store
            .save_beta_features(crate::config::BetaFeatures::default())
            .unwrap();
        assert!(!observer.beta_features().unwrap().delegation);
    }

    #[test]
    fn delegation_beta_off_blocks_new_runs_but_preserves_existing_run_and_replay() {
        let (_dir, store, parent, expert) = fixture();
        let source = Uuid::new_v4();
        store
            .authorize_experts(parent.id, source, "Delegate this", &[expert.id], false)
            .unwrap();
        store
            .save_beta_features(crate::config::BetaFeatures::default())
            .unwrap();
        assert!(store
            .begin_delegation(parent.id, source)
            .unwrap_err()
            .to_string()
            .contains("Settings → Band"));
        assert!(store.load_delegations().unwrap().is_empty());
        store
            .save_beta_features(crate::config::BetaFeatures { delegation: true, ..Default::default() })
            .unwrap();
        let run = store.begin_delegation(parent.id, source).unwrap();
        store
            .save_beta_features(crate::config::BetaFeatures::default())
            .unwrap();
        assert_eq!(
            store.begin_delegation(parent.id, source).unwrap().id,
            run.id
        );
        assert_eq!(store.load_delegation(run.id).unwrap().status, run.status);
        assert!(store.delegation_available_for(parent.id).unwrap());
        assert!(!store.delegation_available_for(Uuid::new_v4()).unwrap());
        store
            .update_delegation(run.id, None, |run| {
                run.status = RunStatus::Cancelled;
                Ok(())
            })
            .unwrap();
        assert!(!store.delegation_available_for(parent.id).unwrap());
        let next = Uuid::new_v4();
        store
            .authorize_experts(parent.id, next, "Next task", &[expert.id], false)
            .unwrap();
        assert!(store
            .begin_delegation(parent.id, next)
            .unwrap_err()
            .to_string()
            .contains("Settings → Band"));
        assert_eq!(store.load_delegations().unwrap().len(), 1);
    }

    fn fixture() -> (tempfile::TempDir, LocalStore, AgentRecord, ExpertProfile) {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
        store
            .save_beta_features(crate::config::BetaFeatures { delegation: true, ..Default::default() })
            .unwrap();
        let project = Project::from_path(dir.path().join("repo"));
        let mut config = AppConfig::default();
        config.projects.push(project.clone());
        store.save_workspace_config(&config).unwrap();
        let model = AgentModel::default_for(AgentKind::Codex);
        let mut agent = AgentRecord::new(
            project.id,
            project.path,
            "Lead",
            "Task",
            AgentKind::Codex,
            model,
            model.default_effort(),
            AgentAccessMode::default(),
        );
        agent.runtime = AgentRuntimeKind::Chat;
        store.save_agents(&[agent.clone()]).unwrap();
        let profile = ExpertProfile {
            id: Uuid::new_v4(),
            revision: 0,
            name: " UI   Designer ".into(),
            description: "Design UI".into(),
            provider: AgentKind::Codex,
            model,
            effort: model.default_effort(),
            instructions: "Design and check the UI".into(),
            skills: vec![],
            expected_outcome: "Checked UI".into(),
            enabled: true,
            archived: false,
            additions: Default::default(),
        };
        let profile = store.save_expert(profile, None).unwrap();
        (dir, store, agent, profile)
    }
    #[test]
    fn natural_submission_prepares_one_run_and_preserves_authority_through_followups() {
        let (_dir, store, agent, profile) = fixture();
        let source = Uuid::new_v4();
        let text = "I want to \"redesign\" the existing video with the current assets and compare several different approaches. Please delagte four on demand teammates without saved profiles.";
        let id = store
            .prepare_delegation_submission(agent.id, source, text, &[], false)
            .unwrap()
            .unwrap();
        let run = store.load_delegation(id).unwrap();
        assert_eq!(run.status, RunStatus::Preparing);
        assert!(run.tasks.is_empty()); // No model calls or semantic task splitting here.
        assert_eq!(run.original_assignment, text);
        assert_eq!(run.authorized_experts, vec![source]);
        assert_eq!(run.temporary_experts[0].profile.model, agent.model);
        assert_eq!(store.load_experts().unwrap(), vec![profile]);
        assert!(store
            .prepare_delegation_submission(
                agent.id,
                Uuid::new_v4(),
                "They can be created on demand without saved profiles",
                &[],
                false
            )
            .unwrap()
            .is_none());
        assert_eq!(
            store.load_delegation(id).unwrap().authorized_experts,
            vec![source]
        );
        assert_eq!(store.load_delegations().unwrap().len(), 1);
        store
            .update_delegation(id, None, |run| {
                run.status = RunStatus::Completed;
                Ok(())
            })
            .unwrap();
        assert!(store
            .prepare_delegation_submission(
                agent.id,
                Uuid::new_v4(),
                "Build an unrelated page",
                &[],
                false
            )
            .unwrap()
            .is_none());
        assert!(store
            .latest_expert_authorization(agent.id)
            .unwrap()
            .unwrap()
            .expert_ids
            .is_empty());
        assert_eq!(store.load_delegations().unwrap().len(), 1);
    }

    #[test]
    fn natural_submission_respects_beta_and_durable_stop() {
        let (_dir, store, agent, _) = fixture();
        store
            .save_beta_features(crate::config::BetaFeatures::default())
            .unwrap();
        assert!(store
            .prepare_delegation_submission(
                agent.id,
                Uuid::new_v4(),
                "Please delegate the review",
                &[],
                false
            )
            .unwrap()
            .is_none());
        assert!(store.load_delegations().unwrap().is_empty());
        store
            .save_beta_features(crate::config::BetaFeatures {
                delegation: true,
                ..Default::default()
            })
            .unwrap();
        let run = store
            .prepare_delegation_submission(
                agent.id,
                Uuid::new_v4(),
                "Please delegate the review",
                &[],
                true,
            )
            .unwrap()
            .unwrap();
        assert!(store.load_delegation(run).unwrap().plan_mode);
        for status in [RunStatus::Paused, RunStatus::Interrupted] {
            store
                .update_delegation(run, None, |run| {
                    run.status = status;
                    Ok(())
                })
                .unwrap();
            let before = store.load_delegation(run).unwrap();
            let source = Uuid::new_v4();
            assert_eq!(
                store
                    .prepare_delegation_submission(
                        agent.id,
                        source,
                        "Add four new teammates",
                        &[],
                        false
                    )
                    .unwrap(),
                Some(run)
            );
            let after = store.load_delegation(run).unwrap();
            assert_eq!(after.status, before.status);
            assert_eq!(after.tasks.len(), before.tasks.len());
            assert!(after.authorized_experts.contains(&source));
            assert!(after.ready_tasks(0).is_empty());
        }
    }

    #[test]
    fn on_demand_wording_works_alongside_a_named_profile() {
        let (_dir, store, agent, profile) = fixture();
        for wording in ["ON-DEMAND", "on demand", "on   demand"] {
            let source = Uuid::new_v4();
            let auth = store
                .authorize_experts(
                    agent.id,
                    source,
                    &format!(
                        "Delegate design to UI Designer and create {wording} teammates for testing"
                    ),
                    &[],
                    false,
                )
                .unwrap()
                .unwrap();
            assert_eq!(auth.expert_ids, vec![profile.id, source]);
            assert_eq!(auth.temporary_experts.len(), 1);
        }
    }

    #[test]
    fn temporary_selection_is_durable_and_does_not_change_saved_profiles() {
        let (dir, store, agent, profile) = fixture();
        let source = Uuid::new_v4();
        let auth = store
            .authorize_experts(
                agent.id,
                source,
                "Research UI Designer alternatives",
                &[Uuid::nil()],
                false,
            )
            .unwrap()
            .unwrap();
        assert_eq!(auth.expert_ids, vec![source]);
        assert_eq!(auth.temporary_experts[0].profile.model, agent.model);
        let run = store.begin_delegation(agent.id, source).unwrap();
        drop(store);
        let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
        let restored = store.load_delegation(run.id).unwrap();
        assert_eq!(restored.temporary_experts, run.temporary_experts);
        assert_eq!(store.load_experts().unwrap(), vec![profile]);
        let mut old = serde_json::to_value(&restored).unwrap();
        old.as_object_mut().unwrap().remove("temporary_experts");
        assert!(serde_json::from_value::<DelegationRun>(old)
            .unwrap()
            .temporary_experts
            .is_empty());
    }

    #[test]
    fn expert_profiles_are_revisioned_and_names_are_unique() {
        let (_dir, store, _agent, p) = fixture();
        assert_eq!(p.name, "UI Designer");
        assert_eq!(p.revision, 1);
        let mut duplicate = p.clone();
        duplicate.id = Uuid::new_v4();
        duplicate.name = "ui designer".into();
        assert!(store.save_expert(duplicate, None).is_err());
        let frozen = p.snapshot().unwrap();
        let mut edit = p.clone();
        edit.instructions = "A different job".into();
        edit.enabled = false;
        let saved = store.save_expert(edit.clone(), Some(1)).unwrap();
        assert_eq!(saved.revision, 2);
        assert!(store.save_expert(edit, Some(1)).is_err());
        assert_eq!(frozen.profile.instructions, p.instructions);
        assert!(saved.snapshot().is_err());
        assert_eq!(store.load_experts().unwrap()[0], saved);
    }
    #[test]
    fn authorization_is_bound_to_submissions_and_empty_requests_clear_discovery() {
        let (_dir, store, agent, p) = fixture();
        let message = Uuid::new_v4();
        assert!(store.begin_delegation(agent.id, message).is_err());
        let grant = store
            .authorize_experts(agent.id, message, "Build using UI Designer", &[], false)
            .unwrap()
            .unwrap();
        assert_eq!(grant.expert_ids, vec![p.id]);
        let run = store.begin_delegation(agent.id, message).unwrap();
        assert_eq!(run.source_message_id, message);
        assert_eq!(
            store.begin_delegation(agent.id, message).unwrap().id,
            run.id
        );
        assert!(store
            .authorize_experts(agent.id, Uuid::new_v4(), "Unrelated question", &[], false)
            .unwrap()
            .is_none());
        assert!(store
            .latest_expert_authorization(agent.id)
            .unwrap()
            .unwrap()
            .expert_ids
            .is_empty());
        assert_eq!(
            store.load_delegation(run.id).unwrap().authorized_experts,
            vec![p.id]
        );
    }
    #[test]
    fn operations_are_scoped_idempotent_and_transactional() {
        let (_dir, store, agent, _p) = fixture();
        let id = Uuid::new_v4();
        store
            .authorize_experts(agent.id, id, "UI Designer: help", &[], false)
            .unwrap();
        let run = store.begin_delegation(agent.id, id).unwrap();
        let input = json!({"action":"test"});
        let answer = json!({"receipt":"same"});
        let first = store
            .delegation_operation(run.id, agent.id, "once", run.revision, &input, |r| {
                r.event(agent.id, None, "test", "Once");
                Ok(answer.clone())
            })
            .unwrap();
        let second = store
            .delegation_operation(run.id, agent.id, "once", run.revision, &input, |_| {
                panic!("Duplicate command executed")
            })
            .unwrap();
        assert_eq!(first, second);
        assert_eq!(
            store.load_delegation(run.id).unwrap().revision,
            run.revision + 1
        );
        assert_eq!(store.load_delegation(run.id).unwrap().events.len(), 1);
        assert!(store
            .delegation_operation(
                run.id,
                agent.id,
                "once",
                run.revision,
                &json!({"action":"different"}),
                |_| Ok(json!({}))
            )
            .is_err());
        assert!(store
            .delegation_operation(
                run.id,
                Uuid::new_v4(),
                "forged",
                run.revision,
                &input,
                |_| Ok(json!({}))
            )
            .is_err());
        assert!(store
            .update_delegation::<()>(run.id, None, |r| {
                r.event(agent.id, None, "bad", "Rollback");
                anyhow::bail!("failed")
            })
            .is_err());
        assert_eq!(store.load_delegation(run.id).unwrap().events.len(), 1);
    }
    #[test]
    fn stopped_runs_remain_readable_but_agent_operations_cannot_restart_them() {
        let (_dir, store, agent, _) = fixture();
        let source = Uuid::new_v4();
        store
            .authorize_experts(agent.id, source, "UI Designer", &[], false)
            .unwrap();
        let run = store.begin_delegation(agent.id, source).unwrap();
        for status in [
            RunStatus::Paused,
            RunStatus::Interrupted,
            RunStatus::Blocked,
        ] {
            store
                .update_delegation(run.id, None, |r| {
                    r.status = status;
                    Ok(())
                })
                .unwrap();
            let saved = store.load_delegation(run.id).unwrap();
            let error = store
                .delegation_operation(
                    saved.id,
                    agent.id,
                    "wait",
                    saved.revision,
                    &json!({"tool":"delegation_wait"}),
                    |r| {
                        r.status = RunStatus::Waiting;
                        Ok(json!({}))
                    },
                )
                .unwrap_err();
            assert!(error.to_string().contains("cannot resume"));
            let after = store.load_delegation(run.id).unwrap();
            assert_eq!(after.status, status);
            assert_eq!(after.revision, saved.revision);
            assert!(after.ready_tasks(0).is_empty());
            let next = Uuid::new_v4();
            store
                .authorize_experts(agent.id, next, "UI Designer", &[], false)
                .unwrap();
            assert!(store.begin_delegation(agent.id, next).is_err());
        }
    }

    #[test]
    fn paused_state_and_single_coordinator_lease_survive_reopen() {
        let (dir, store, agent, _) = fixture();
        let source = Uuid::new_v4();
        store
            .authorize_experts(agent.id, source, "UI Designer", &[], false)
            .unwrap();
        let run = store.begin_delegation(agent.id, source).unwrap();
        let owner = Uuid::new_v4();
        assert!(store.claim_delegation_coordinator(owner).unwrap());
        assert!(store.claim_delegation_coordinator(owner).unwrap());
        assert!(!store.claim_delegation_coordinator(Uuid::new_v4()).unwrap());
        store
            .update_delegation(run.id, None, |r| {
                r.pause("Stopped", false);
                Ok(())
            })
            .unwrap();
        drop(store);
        let reopened = LocalStore::open(dir.path().to_path_buf()).unwrap();
        let saved = reopened.load_delegation(run.id).unwrap();
        assert_eq!(saved.status, RunStatus::Paused);
        assert!(saved.ready_tasks(0).is_empty());
        assert!(reopened
            .delegation_operation(
                saved.id,
                agent.id,
                "late",
                saved.revision,
                &json!({}),
                |r| {
                    r.status = RunStatus::Active;
                    Ok(json!({}))
                }
            )
            .is_err());
    }
    #[test]
    fn archive_restores_private_dirty_work_and_never_activates_imported_runs() {
        use crate::delegation::{
            workspace, DelegationAttempt, DelegationBinding, TaskKind, TaskPlan, TaskStatus,
        };
        let (dir, store, parent, mut p) = fixture();
        p.additions.bundled_skills.push("impeccable".into());
        p = store.save_expert(p.clone(), Some(p.revision)).unwrap();
        std::fs::create_dir_all(&parent.project_path).unwrap();
        git2::Repository::init(&parent.project_path).unwrap();
        std::fs::write(parent.project_path.join("todo.txt"), "user work\n").unwrap();
        let source = Uuid::new_v4();
        store
            .authorize_experts(parent.id, source, "Use UI Designer", &[], false)
            .unwrap();
        let run = store.begin_delegation(parent.id, source).unwrap();
        let expert = p.snapshot().unwrap();
        store
            .update_delegation(run.id, None, |r| {
                r.add_plans(
                    parent.id,
                    vec![TaskPlan {
                        model_request: None,
                        key: "ui".into(),
                        expert_id: p.id,
                        goal: "UI".into(),
                        brief: "Improve UI".into(),
                        expected_outcome: "Checked".into(),
                        repository: parent.project_path.clone(),
                        dependencies: vec![],
                        kind: TaskKind::Implementation,
                        held: false,
                    }],
                    &[expert.clone()],
                )
            })
            .unwrap();
        let run = store.load_delegation(run.id).unwrap();
        let task_id = run.tasks[0].id;
        let attempt = Uuid::new_v4();
        let storage = store
            .app_data_dir()
            .join("delegation")
            .join(run.id.to_string());
        let working = storage.join(format!("attempt-{attempt}"));
        let base = workspace::capture(&parent.project_path, &storage).unwrap();
        workspace::materialize(&base, &working).unwrap();
        std::fs::write(working.join("todo.txt"), "expert unfinished work\n").unwrap();
        std::fs::write(working.join(".env"), "SECRET=do-not-export").unwrap();
        let mut child = parent.clone();
        child.id = Uuid::new_v4();
        child.expert_snapshot = Some(expert);
        child.delegation = Some(DelegationBinding {
            run_id: run.id,
            parent_agent_id: parent.id,
            task_id: Some(task_id),
            attempt_id: Some(attempt),
            workspace: Some(working.clone()),
            task_kind: Some(TaskKind::Implementation),
        });
        store.save_agents(&[parent.clone(), child.clone()]).unwrap();
        store
            .update_delegation(run.id, None, |r| {
                r.status = RunStatus::Active;
                let t = r.task_mut(task_id)?;
                t.status = TaskStatus::Running;
                t.attempts.push(DelegationAttempt {
                    id: attempt,
                    child_agent_id: child.id,
                    generation: 0,
                    workspace: working,
                    snapshot: Some(base),
                    completed_snapshot: None,
                    archive_snapshot: None,
                    working_copy_cleaned: false,
                    result: None,
                    result_revision: None,
                    report_requested: false,
                    session_id: None,
                    progress: "Editing".into(),
                    usage: None,
                });
                Ok(())
            })
            .unwrap();
        let archive = dir.path().join("archive.zip");
        store.export_workspace(&archive).unwrap();
        let restored_dir = tempfile::tempdir().unwrap();
        let restored = LocalStore::open(restored_dir.path().to_path_buf()).unwrap();
        restored.import_workspace_replace(&archive).unwrap();
        let imported = restored.load_delegation(run.id).unwrap();
        assert_eq!(imported.status, RunStatus::Interrupted);
        let frozen = &imported.tasks[0].expert;
        assert!(frozen.skills[0].files.contains_key("reference/review.md"));
        assert!(frozen
            .runtime_instructions(&restored.root().join("expert-skill-cache"))
            .is_ok());
        let imported_child = restored
            .load_agents()
            .unwrap()
            .into_iter()
            .find(|a| a.id == child.id)
            .unwrap();
        assert_eq!(imported_child.expert_snapshot.as_ref(), Some(frozen));
        assert_eq!(
            restored
                .load_experts()
                .unwrap()
                .into_iter()
                .find(|e| e.id == p.id)
                .unwrap(),
            p
        );
        assert!(imported.ready_tasks(0).is_empty());
        let a = imported.tasks[0].attempt().unwrap();
        assert!(a.workspace.starts_with(restored.app_data_dir()));
        assert_eq!(
            std::fs::read_to_string(a.workspace.join("todo.txt")).unwrap(),
            "expert unfinished work\n"
        );
        assert!(!a.workspace.join(".env").exists());
        assert!(git2::Repository::open(&a.workspace).is_ok());
        assert_eq!(
            std::fs::read_to_string(parent.project_path.join("todo.txt")).unwrap(),
            "user work\n"
        );
        assert!(restored.delegation_cleanup_preview(run.id).is_err());
    }
}
