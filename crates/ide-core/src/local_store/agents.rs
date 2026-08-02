use super::*;

pub(super) async fn save_agents_async(conn: &Connection, agents: &[AgentRecord]) -> Result<()> {
    let ids: HashSet<String> = agents.iter().map(|agent| agent.id.to_string()).collect();
    delete_missing(conn, "agents", &ids).await?;
    for agent in agents {
        let source_doc = agent
            .source_doc
            .as_deref()
            .map(crate::branding::migrate_legacy_doc_path);
        conn.execute(
            "INSERT INTO agents
            (id, project_id, project_path, title, doc, notes, status, provider, runtime, model,
             effort, access_mode, source_doc, hidden_doc_assistant, cli_session_id,
             chat_session_id, ship_pr_repo_path, ship_pr_branch, created_at, updated_at, started_at,
             external_model_id, external_model_label, external_model_variants,
             lane_path, solo_branch, solo_base_branch, solo_rejoined_branch, lane_profile,
             verification_completed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
             ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30)
             ON CONFLICT(id) DO UPDATE SET
                 project_id = excluded.project_id,
                 project_path = excluded.project_path,
                 title = excluded.title,
                 doc = excluded.doc,
                 notes = excluded.notes,
                 status = excluded.status,
                 provider = excluded.provider,
                 runtime = excluded.runtime,
                 model = excluded.model,
                 effort = excluded.effort,
                 access_mode = excluded.access_mode,
                 source_doc = excluded.source_doc,
                 hidden_doc_assistant = excluded.hidden_doc_assistant,
                 cli_session_id = excluded.cli_session_id,
                 chat_session_id = excluded.chat_session_id,
                 ship_pr_repo_path = excluded.ship_pr_repo_path,
                 ship_pr_branch = excluded.ship_pr_branch,
                 created_at = excluded.created_at,
                 updated_at = excluded.updated_at,
                 started_at = excluded.started_at,
                 external_model_id = excluded.external_model_id,
                 external_model_label = excluded.external_model_label,
                 external_model_variants = excluded.external_model_variants,
                 lane_path = excluded.lane_path,
                 solo_branch = excluded.solo_branch,
                 solo_base_branch = excluded.solo_base_branch,
                 solo_rejoined_branch = excluded.solo_rejoined_branch,
                 lane_profile = excluded.lane_profile,
                 verification_completed_at = excluded.verification_completed_at",
            params![
                agent.id.to_string(),
                agent.project_id.0.to_string(),
                path_to_string(&agent.project_path),
                agent.title.as_str(),
                agent.doc.as_str(),
                agent.notes.as_str(),
                serde_label(&agent.status)?,
                serde_label(&agent.provider)?,
                serde_label(&agent.runtime)?,
                serde_label(&agent.model)?,
                serde_label(&agent.effort)?,
                serde_label(&agent.access_mode)?,
                opt_path_to_string(source_doc.as_deref()),
                bool_to_i64(agent.hidden_doc_assistant),
                agent.cli_session_id.clone(),
                agent.chat_session_id.clone(),
                opt_path_to_string(agent.ship_pr_repo_path.as_deref()),
                agent.ship_pr_branch.clone(),
                u64_to_i64(agent.created_at)?,
                u64_to_i64(agent.updated_at)?,
                agent.started_at.map(u64_to_i64).transpose()?,
                agent.external_model_id.clone(),
                agent.external_model_label.clone(),
                serde_json::to_string(&agent.external_model_variants)?,
                opt_path_to_string(agent.lane_path.as_deref()),
                agent.solo_branch.clone(),
                agent.solo_base_branch.clone(),
                agent.solo_rejoined_branch.clone(),
                agent.lane_profile.map(LaneProfile::as_str),
                agent
                    .verification_completed_at
                    .map(u64_to_i64)
                    .transpose()?,
            ],
        )
        .await?;
        conn.execute(
            "DELETE FROM agent_linked_docs WHERE agent_id = ?1",
            [agent.id.to_string()],
        )
        .await?;
        for (index, path) in agent.linked_docs.iter().enumerate() {
            let path = crate::branding::migrate_legacy_doc_path(path);
            conn.execute(
                "INSERT INTO agent_linked_docs (agent_id, path, sort_order) VALUES (?1, ?2, ?3)",
                (agent.id.to_string(), path_to_string(&path), index as i64),
            )
            .await?;
        }
        conn.execute(
            "DELETE FROM agent_linked_tasks WHERE agent_id = ?1",
            [agent.id.to_string()],
        )
        .await?;
        let mut linked_tasks = agent.linked_tasks.clone();
        if let Some(source_task) = agent.source_task.clone() {
            if !linked_tasks
                .iter()
                .any(|task| task.same_issue(&source_task))
            {
                linked_tasks.push(source_task);
            }
        }
        for (index, task) in linked_tasks.iter().enumerate() {
            let is_source = agent
                .source_task
                .as_ref()
                .is_some_and(|source| source.same_issue(task));
            conn.execute(
                "INSERT INTO agent_linked_tasks
                 (agent_id, provider, site_url, issue_id, issue_key, issue_url, title,
                  is_source, sort_order)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    agent.id.to_string(),
                    serde_label(&task.provider)?,
                    task.site_url.as_str(),
                    task.issue_id.as_str(),
                    task.issue_key.as_str(),
                    task.issue_url.as_str(),
                    task.title.as_str(),
                    bool_to_i64(is_source),
                    index as i64,
                ],
            )
            .await?;
        }
        conn.execute(
            "DELETE FROM agent_changed_files WHERE agent_id = ?1",
            [agent.id.to_string()],
        )
        .await?;
        for file in &agent.changed_files {
            conn.execute(
                "INSERT INTO agent_changed_files (agent_id, path, additions, deletions)
                 VALUES (?1, ?2, ?3, ?4)",
                (
                    agent.id.to_string(),
                    path_to_string(&file.path),
                    file.additions as i64,
                    file.deletions as i64,
                ),
            )
            .await?;
        }
        if let Some(session_id) = &agent.cli_session_id {
            upsert_runtime_session(conn, agent.id, "cli", session_id, agent.updated_at).await?;
        }
        if let Some(session_id) = &agent.chat_session_id {
            upsert_runtime_session(conn, agent.id, "chat", session_id, agent.updated_at).await?;
        }
    }
    Ok(())
}

pub(super) async fn load_agents_async(conn: &Connection) -> Result<Vec<AgentRecord>> {
    let linked_docs = load_linked_docs(conn).await?;
    let linked_tasks = load_linked_tasks(conn).await?;
    let changed_files = load_changed_files(conn).await?;
    let mut rows = conn
        .query(
            "SELECT id, project_id, project_path, title, doc, notes, status, provider, runtime,
             model, effort, access_mode, source_doc, hidden_doc_assistant, cli_session_id,
             chat_session_id, ship_pr_repo_path, ship_pr_branch, created_at, updated_at, started_at,
             external_model_id, external_model_label, external_model_variants,
             lane_path, solo_branch, solo_base_branch, solo_rejoined_branch, lane_profile,
             verification_completed_at
             FROM agents ORDER BY updated_at DESC",
            (),
        )
        .await?;
    let mut agents = Vec::new();
    while let Some(row) = rows.next().await? {
        let id = parse_uuid(&row.get::<String>(0)?)?;
        agents.push(AgentRecord {
            id,
            project_id: ProjectId(parse_uuid(&row.get::<String>(1)?)?),
            project_path: PathBuf::from(row.get::<String>(2)?),
            title: row.get(3)?,
            doc: row.get(4)?,
            notes: row.get(5)?,
            status: serde_parse(&row.get::<String>(6)?)?,
            provider: serde_parse(&row.get::<String>(7)?)?,
            runtime: serde_parse(&row.get::<String>(8)?)?,
            model: serde_parse(&row.get::<String>(9)?)?,
            effort: serde_parse(&row.get::<String>(10)?)?,
            access_mode: serde_parse(&row.get::<String>(11)?)?,
            source_doc: opt_text(&row, 12)?.map(PathBuf::from),
            hidden_doc_assistant: row.get::<i64>(13)? != 0,
            design_context: None,
            cli_session_id: opt_text(&row, 14)?,
            chat_session_id: opt_text(&row, 15)?,
            ship_pr_repo_path: opt_text(&row, 16)?.map(PathBuf::from),
            ship_pr_branch: opt_text(&row, 17)?,
            created_at: i64_to_u64(row.get(18)?)?,
            updated_at: i64_to_u64(row.get(19)?)?,
            started_at: opt_i64(&row, 20)?.map(i64_to_u64).transpose()?,
            external_model_id: opt_text(&row, 21)?,
            external_model_label: opt_text(&row, 22)?,
            external_model_variants: serde_json::from_str(&row.get::<String>(23)?)
                .unwrap_or_default(),
            lane_path: opt_text(&row, 24)?.map(PathBuf::from),
            solo_branch: opt_text(&row, 25)?,
            solo_base_branch: opt_text(&row, 26)?,
            solo_rejoined_branch: opt_text(&row, 27)?,
            lane_profile: opt_text(&row, 28)?
                .as_deref()
                .and_then(LaneProfile::parse_str),
            verification_completed_at: opt_i64(&row, 29)?.map(i64_to_u64).transpose()?,
            linked_docs: linked_docs.get(&id).cloned().unwrap_or_default(),
            linked_tasks: linked_tasks.linked.get(&id).cloned().unwrap_or_default(),
            source_task: linked_tasks.source.get(&id).cloned(),
            changed_files: changed_files.get(&id).cloned().unwrap_or_default(),
        });
    }
    Ok(agents)
}

pub(super) async fn load_linked_docs(conn: &Connection) -> Result<HashMap<Uuid, Vec<PathBuf>>> {
    let mut rows = conn
        .query(
            "SELECT agent_id, path FROM agent_linked_docs ORDER BY agent_id ASC, sort_order ASC",
            (),
        )
        .await?;
    let mut linked: HashMap<Uuid, Vec<PathBuf>> = HashMap::new();
    while let Some(row) = rows.next().await? {
        let agent_id = parse_uuid(&row.get::<String>(0)?)?;
        linked
            .entry(agent_id)
            .or_default()
            .push(PathBuf::from(row.get::<String>(1)?));
    }
    Ok(linked)
}

#[derive(Default)]
pub(super) struct LoadedLinkedTasks {
    linked: HashMap<Uuid, Vec<TaskRef>>,
    source: HashMap<Uuid, TaskRef>,
}

pub(super) async fn load_linked_tasks(conn: &Connection) -> Result<LoadedLinkedTasks> {
    let mut rows = conn
        .query(
            "SELECT agent_id, provider, site_url, issue_id, issue_key, issue_url, title, is_source
             FROM agent_linked_tasks ORDER BY agent_id ASC, sort_order ASC",
            (),
        )
        .await?;
    let mut loaded = LoadedLinkedTasks::default();
    while let Some(row) = rows.next().await? {
        let agent_id = parse_uuid(&row.get::<String>(0)?)?;
        let task = TaskRef {
            provider: serde_parse(&row.get::<String>(1)?)?,
            site_url: row.get(2)?,
            issue_id: row.get(3)?,
            issue_key: row.get(4)?,
            issue_url: row.get(5)?,
            title: row.get(6)?,
        };
        if row.get::<i64>(7)? != 0 {
            loaded.source.insert(agent_id, task.clone());
        }
        loaded.linked.entry(agent_id).or_default().push(task);
    }
    Ok(loaded)
}

pub(super) async fn load_changed_files(
    conn: &Connection,
) -> Result<HashMap<Uuid, Vec<AgentChangedFile>>> {
    let mut rows = conn
        .query(
            "SELECT agent_id, path, additions, deletions FROM agent_changed_files
             ORDER BY agent_id ASC, path ASC",
            (),
        )
        .await?;
    let mut files: HashMap<Uuid, Vec<AgentChangedFile>> = HashMap::new();
    while let Some(row) = rows.next().await? {
        let agent_id = parse_uuid(&row.get::<String>(0)?)?;
        files.entry(agent_id).or_default().push(AgentChangedFile {
            path: PathBuf::from(row.get::<String>(1)?),
            additions: i64_to_usize(row.get(2)?)?,
            deletions: i64_to_usize(row.get(3)?)?,
        });
    }
    Ok(files)
}

pub(super) async fn upsert_runtime_session(
    conn: &Connection,
    agent_id: Uuid,
    kind: &str,
    session_id: &str,
    updated_at: u64,
) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO agent_runtime_sessions
         (agent_id, kind, session_id, updated_at) VALUES (?1, ?2, ?3, ?4)",
        (
            agent_id.to_string(),
            kind,
            session_id,
            u64_to_i64(updated_at)?,
        ),
    )
    .await?;
    Ok(())
}
