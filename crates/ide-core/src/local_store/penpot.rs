//! Legacy archive compatibility only; no live design integration.
use super::*;
#[cfg(test)]
use crate::DocAssistantRecord;
use crate::{AgentAccessMode, AgentEffort, AgentKind, AgentModel};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredPenpotConnection {
    pub id: Uuid,
    pub instance_url: String,
    pub mcp_url: String,
    pub profile_id: Option<Uuid>,
    pub profile_email: Option<String>,
    pub default_team_id: Option<Uuid>,
    pub default_project_id: Option<Uuid>,
    pub is_active: bool,
    pub verified_at: Option<u64>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredProjectPenpotBinding {
    pub project_id: ProjectId,
    pub connection_id: Uuid,
    pub penpot_team_id: Uuid,
    pub penpot_project_id: Uuid,
    pub selected_design_id: Option<Uuid>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredPenpotDesign {
    /// Stable Choro-local design id.
    pub id: Uuid,
    pub project_id: ProjectId,
    pub connection_id: Uuid,
    /// Penpot's remote file id.
    pub penpot_file_id: Uuid,
    pub penpot_project_id: Uuid,
    pub penpot_team_id: Uuid,
    pub name: String,
    pub page_id: Option<Uuid>,
    #[serde(default)]
    pub source_doc: Option<PathBuf>,
    #[serde(default)]
    pub source_task: Option<TaskRef>,
    pub last_synced_at: Option<u64>,
    pub archived_at: Option<u64>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredPenpotDesignConversation {
    pub id: Uuid,
    pub design_id: Uuid,
    pub agent_id: Uuid,
    pub ordinal: i64,
    pub title: String,
    pub provider: AgentKind,
    pub model: AgentModel,
    pub external_model_id: Option<String>,
    pub external_model_label: Option<String>,
    pub external_model_variants: Vec<String>,
    pub effort: AgentEffort,
    pub access_mode: AgentAccessMode,
    pub chat_session_id: Option<String>,
    pub cli_session_id: Option<String>,
    pub last_transcript_path: Option<PathBuf>,
    pub is_current: bool,
    pub created_at: u64,
    pub updated_at: u64,
    pub deleted_at: Option<u64>,
}

#[cfg(test)]
impl LocalStore {
    pub fn active_penpot_connection(&self) -> Result<Option<StoredPenpotConnection>> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            load_active_penpot_connection_async(&conn).await
        })
    }

    pub fn penpot_connection_for_profile(
        &self,
        instance_url: &str,
        profile_id: Uuid,
    ) -> Result<Option<StoredPenpotConnection>> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            let mut rows = conn
                .query(
                    "SELECT id, instance_url, mcp_url, profile_id, profile_email, default_team_id,
                            default_project_id, is_active, verified_at, created_at, updated_at
                     FROM penpot_connections
                     WHERE instance_url = ?1 AND profile_id = ?2
                     ORDER BY updated_at DESC LIMIT 1",
                    params![instance_url, profile_id.to_string()],
                )
                .await?;
            rows.next()
                .await?
                .map(|row| penpot_connection_from_row(&row))
                .transpose()
        })
    }

    pub fn save_active_penpot_connection(&self, connection: &StoredPenpotConnection) -> Result<()> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            execute_transaction(&conn, |conn| {
                Box::pin(async move {
                    conn.execute(
                        "UPDATE penpot_connections SET is_active = 0 WHERE is_active = 1",
                        (),
                    )
                    .await?;
                    insert_penpot_connection_async(conn, connection, true).await
                })
            })
            .await
        })
    }

    pub fn update_penpot_connection_profile(
        &self,
        connection_id: Uuid,
        profile_id: Option<Uuid>,
        profile_email: Option<&str>,
        default_team_id: Uuid,
        default_project_id: Uuid,
    ) -> Result<()> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            let now = unix_now();
            conn.execute(
                "UPDATE penpot_connections
                 SET profile_id = ?2, profile_email = ?3, default_team_id = ?4,
                     default_project_id = ?5, verified_at = ?6, updated_at = ?6
                 WHERE id = ?1",
                params![
                    connection_id.to_string(),
                    profile_id.map(|id| id.to_string()),
                    profile_email,
                    default_team_id.to_string(),
                    default_project_id.to_string(),
                    u64_to_i64(now)?,
                ],
            )
            .await?;
            Ok(())
        })
    }

    pub fn upsert_project_penpot_binding(
        &self,
        binding: &StoredProjectPenpotBinding,
    ) -> Result<()> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            insert_project_penpot_binding_async(&conn, binding).await
        })
    }

    pub fn project_penpot_binding(
        &self,
        project_id: ProjectId,
        connection_id: Uuid,
    ) -> Result<Option<StoredProjectPenpotBinding>> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            load_project_penpot_binding_async(&conn, project_id, connection_id).await
        })
    }

    pub fn upsert_penpot_design(&self, design: &StoredPenpotDesign) -> Result<()> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            insert_penpot_design_async(&conn, design).await
        })
    }

    pub fn load_penpot_designs(&self, project_id: ProjectId) -> Result<Vec<StoredPenpotDesign>> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            load_penpot_designs_async(&conn, Some(project_id)).await
        })
    }

    pub fn load_all_penpot_designs(&self) -> Result<Vec<StoredPenpotDesign>> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            load_penpot_designs_async(&conn, None).await
        })
    }

    pub fn delete_penpot_design(&self, project_id: ProjectId, design_id: Uuid) -> Result<()> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            execute_transaction(&conn, |conn| {
                Box::pin(async move {
                    conn.execute(
                        "UPDATE project_penpot_bindings
                         SET selected_design_id = NULL, updated_at = ?3
                         WHERE project_id = ?1 AND selected_design_id = ?2",
                        params![
                            project_id.0.to_string(),
                            design_id.to_string(),
                            u64_to_i64(unix_now())?,
                        ],
                    )
                    .await?;
                    conn.execute(
                        "DELETE FROM penpot_designs WHERE project_id = ?1 AND id = ?2",
                        params![project_id.0.to_string(), design_id.to_string()],
                    )
                    .await?;
                    Ok(())
                })
            })
            .await
        })
    }

    pub fn select_penpot_design(&self, project_id: ProjectId, design_id: Uuid) -> Result<()> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            conn.execute(
                "UPDATE project_penpot_bindings
                 SET selected_design_id = ?2, updated_at = ?3
                 WHERE project_id = ?1
                   AND connection_id = (SELECT connection_id FROM penpot_designs WHERE id = ?2)",
                params![
                    project_id.0.to_string(),
                    design_id.to_string(),
                    u64_to_i64(unix_now())?,
                ],
            )
            .await?;
            Ok(())
        })
    }

    pub fn ensure_current_penpot_conversation(
        &self,
        design_id: Uuid,
    ) -> Result<StoredPenpotDesignConversation> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            if let Some(current) = load_current_penpot_conversation_async(&conn, design_id).await? {
                return Ok(current);
            }
            create_penpot_conversation_async(&conn, design_id, None).await
        })
    }

    pub fn create_penpot_conversation(
        &self,
        design_id: Uuid,
    ) -> Result<StoredPenpotDesignConversation> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            create_penpot_conversation_async(&conn, design_id, None).await
        })
    }

    pub fn import_legacy_penpot_conversation(
        &self,
        design_id: Uuid,
        legacy: &DocAssistantRecord,
    ) -> Result<StoredPenpotDesignConversation> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            if let Some(current) = load_current_penpot_conversation_async(&conn, design_id).await? {
                return Ok(current);
            }
            let conversation = StoredPenpotDesignConversation {
                id: Uuid::new_v4(),
                design_id,
                agent_id: legacy.chat_agent_id,
                ordinal: 1,
                title: "Conversation 1".to_string(),
                provider: legacy.provider,
                model: legacy.model.clone(),
                external_model_id: legacy.external_model_id.clone(),
                external_model_label: legacy.external_model_label.clone(),
                external_model_variants: legacy.external_model_variants.clone(),
                effort: legacy.effort,
                access_mode: legacy.access_mode,
                chat_session_id: legacy.chat_session_id.clone(),
                cli_session_id: legacy.cli_session_id.clone(),
                last_transcript_path: legacy.last_transcript_path.clone(),
                is_current: true,
                created_at: legacy.created_at,
                updated_at: legacy.updated_at,
                deleted_at: None,
            };
            insert_penpot_conversation_async(&conn, &conversation).await?;
            Ok(conversation)
        })
    }

    pub fn load_penpot_conversations(
        &self,
        design_id: Uuid,
    ) -> Result<Vec<StoredPenpotDesignConversation>> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            load_penpot_conversations_async(&conn, design_id).await
        })
    }

    pub fn select_penpot_conversation(&self, design_id: Uuid, conversation_id: Uuid) -> Result<()> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            execute_transaction(&conn, |conn| {
                Box::pin(async move {
                    conn.execute(
                        "UPDATE penpot_design_conversations SET is_current = 0
                         WHERE design_id = ?1",
                        [design_id.to_string()],
                    )
                    .await?;
                    conn.execute(
                        "UPDATE penpot_design_conversations
                         SET is_current = 1, updated_at = ?3
                         WHERE id = ?1 AND design_id = ?2 AND deleted_at IS NULL",
                        params![
                            conversation_id.to_string(),
                            design_id.to_string(),
                            u64_to_i64(unix_now())?,
                        ],
                    )
                    .await?;
                    Ok(())
                })
            })
            .await
        })
    }

    pub fn update_penpot_conversation_runtime(
        &self,
        agent_id: Uuid,
        provider: AgentKind,
        model: AgentModel,
        external_model_id: Option<&str>,
        external_model_label: Option<&str>,
        external_model_variants: &[String],
        effort: AgentEffort,
        access_mode: AgentAccessMode,
        chat_session_id: Option<&str>,
        cli_session_id: Option<&str>,
        last_transcript_path: Option<&Path>,
    ) -> Result<()> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            conn.execute(
                "UPDATE penpot_design_conversations
                 SET provider = ?2, model = ?3, external_model_id = ?4,
                     external_model_label = ?5, external_model_variants = ?6,
                     effort = ?7, access_mode = ?8,
                     chat_session_id = ?9, cli_session_id = ?10,
                     last_transcript_path = ?11, updated_at = ?12
                 WHERE agent_id = ?1",
                params![
                    agent_id.to_string(),
                    serde_label(&provider)?,
                    serde_label(&model)?,
                    external_model_id,
                    external_model_label,
                    serde_json::to_string(external_model_variants)?,
                    serde_label(&effort)?,
                    serde_label(&access_mode)?,
                    chat_session_id,
                    cli_session_id,
                    opt_path_to_string(last_transcript_path),
                    u64_to_i64(unix_now())?,
                ],
            )
            .await?;
            Ok(())
        })
    }
}

pub(super) async fn insert_penpot_connection_async(
    conn: &Connection,
    value: &StoredPenpotConnection,
    force_active: bool,
) -> Result<()> {
    conn.execute(
        "INSERT INTO penpot_connections
         (id, instance_url, mcp_url, profile_id, profile_email, default_team_id,
          default_project_id, is_active, verified_at, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         ON CONFLICT(id) DO UPDATE SET
           instance_url = excluded.instance_url,
           mcp_url = excluded.mcp_url,
           profile_id = excluded.profile_id,
           profile_email = excluded.profile_email,
           default_team_id = excluded.default_team_id,
           default_project_id = excluded.default_project_id,
           is_active = excluded.is_active,
           verified_at = excluded.verified_at,
           updated_at = excluded.updated_at",
        params![
            value.id.to_string(),
            value.instance_url.as_str(),
            value.mcp_url.as_str(),
            value.profile_id.map(|id| id.to_string()),
            value.profile_email.as_deref(),
            value.default_team_id.map(|id| id.to_string()),
            value.default_project_id.map(|id| id.to_string()),
            bool_to_i64(force_active || value.is_active),
            value.verified_at.map(u64_to_i64).transpose()?,
            u64_to_i64(value.created_at)?,
            u64_to_i64(value.updated_at)?,
        ],
    )
    .await?;
    Ok(())
}

#[cfg(test)]
pub(super) async fn load_active_penpot_connection_async(
    conn: &Connection,
) -> Result<Option<StoredPenpotConnection>> {
    let mut rows = conn
        .query(
            "SELECT id, instance_url, mcp_url, profile_id, profile_email, default_team_id,
                    default_project_id, is_active, verified_at, created_at, updated_at
             FROM penpot_connections WHERE is_active = 1 LIMIT 1",
            (),
        )
        .await?;
    rows.next()
        .await?
        .map(|row| penpot_connection_from_row(&row))
        .transpose()
}

pub(super) async fn load_all_penpot_connections_async(
    conn: &Connection,
) -> Result<Vec<StoredPenpotConnection>> {
    let mut rows = conn
        .query(
            "SELECT id, instance_url, mcp_url, profile_id, profile_email, default_team_id,
                    default_project_id, is_active, verified_at, created_at, updated_at
             FROM penpot_connections ORDER BY created_at ASC",
            (),
        )
        .await?;
    let mut values = Vec::new();
    while let Some(row) = rows.next().await? {
        values.push(penpot_connection_from_row(&row)?);
    }
    Ok(values)
}

fn penpot_connection_from_row(row: &turso::Row) -> Result<StoredPenpotConnection> {
    Ok(StoredPenpotConnection {
        id: parse_uuid(&row.get::<String>(0)?)?,
        instance_url: row.get(1)?,
        mcp_url: row.get(2)?,
        profile_id: opt_text(row, 3)?.map(|v| parse_uuid(&v)).transpose()?,
        profile_email: opt_text(row, 4)?,
        default_team_id: opt_text(row, 5)?.map(|v| parse_uuid(&v)).transpose()?,
        default_project_id: opt_text(row, 6)?.map(|v| parse_uuid(&v)).transpose()?,
        is_active: row.get::<i64>(7)? != 0,
        verified_at: opt_i64(row, 8)?.map(i64_to_u64).transpose()?,
        created_at: i64_to_u64(row.get(9)?)?,
        updated_at: i64_to_u64(row.get(10)?)?,
    })
}

pub(super) async fn insert_project_penpot_binding_async(
    conn: &Connection,
    value: &StoredProjectPenpotBinding,
) -> Result<()> {
    conn.execute(
        "INSERT INTO project_penpot_bindings
         (project_id, connection_id, penpot_team_id, penpot_project_id, selected_design_id,
          created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(project_id, connection_id) DO UPDATE SET
           penpot_team_id = excluded.penpot_team_id,
           penpot_project_id = excluded.penpot_project_id,
           selected_design_id = COALESCE(excluded.selected_design_id, project_penpot_bindings.selected_design_id),
           updated_at = excluded.updated_at",
        params![
            value.project_id.0.to_string(),
            value.connection_id.to_string(),
            value.penpot_team_id.to_string(),
            value.penpot_project_id.to_string(),
            value.selected_design_id.map(|id| id.to_string()),
            u64_to_i64(value.created_at)?,
            u64_to_i64(value.updated_at)?,
        ],
    )
    .await?;
    Ok(())
}

#[cfg(test)]
pub(super) async fn load_project_penpot_binding_async(
    conn: &Connection,
    project_id: ProjectId,
    connection_id: Uuid,
) -> Result<Option<StoredProjectPenpotBinding>> {
    let mut rows = conn
        .query(
            "SELECT project_id, connection_id, penpot_team_id, penpot_project_id,
                    selected_design_id, created_at, updated_at
             FROM project_penpot_bindings WHERE project_id = ?1 AND connection_id = ?2",
            params![project_id.0.to_string(), connection_id.to_string()],
        )
        .await?;
    rows.next()
        .await?
        .map(|row| project_penpot_binding_from_row(&row))
        .transpose()
}

fn project_penpot_binding_from_row(row: &turso::Row) -> Result<StoredProjectPenpotBinding> {
    Ok(StoredProjectPenpotBinding {
        project_id: ProjectId(parse_uuid(&row.get::<String>(0)?)?),
        connection_id: parse_uuid(&row.get::<String>(1)?)?,
        penpot_team_id: parse_uuid(&row.get::<String>(2)?)?,
        penpot_project_id: parse_uuid(&row.get::<String>(3)?)?,
        selected_design_id: opt_text(row, 4)?.map(|v| parse_uuid(&v)).transpose()?,
        created_at: i64_to_u64(row.get(5)?)?,
        updated_at: i64_to_u64(row.get(6)?)?,
    })
}

pub(super) async fn load_all_project_penpot_bindings_async(
    conn: &Connection,
) -> Result<Vec<StoredProjectPenpotBinding>> {
    let mut rows = conn
        .query(
            "SELECT project_id, connection_id, penpot_team_id, penpot_project_id,
                    selected_design_id, created_at, updated_at
             FROM project_penpot_bindings ORDER BY created_at ASC",
            (),
        )
        .await?;
    let mut values = Vec::new();
    while let Some(row) = rows.next().await? {
        values.push(project_penpot_binding_from_row(&row)?);
    }
    Ok(values)
}

pub(super) async fn insert_penpot_design_async(
    conn: &Connection,
    value: &StoredPenpotDesign,
) -> Result<()> {
    conn.execute(
        "INSERT INTO penpot_designs
         (id, project_id, connection_id, penpot_file_id, penpot_project_id, penpot_team_id,
          name, page_id, source_doc, source_task_json, last_synced_at, archived_at,
          created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
         ON CONFLICT(id) DO UPDATE SET
           name = excluded.name,
           page_id = excluded.page_id,
           source_doc = excluded.source_doc,
           source_task_json = excluded.source_task_json,
           last_synced_at = excluded.last_synced_at,
           archived_at = excluded.archived_at,
           updated_at = excluded.updated_at",
        params![
            value.id.to_string(),
            value.project_id.0.to_string(),
            value.connection_id.to_string(),
            value.penpot_file_id.to_string(),
            value.penpot_project_id.to_string(),
            value.penpot_team_id.to_string(),
            value.name.as_str(),
            value.page_id.map(|id| id.to_string()),
            value
                .source_doc
                .as_ref()
                .map(|path| path.to_string_lossy().to_string()),
            value
                .source_task
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?,
            value.last_synced_at.map(u64_to_i64).transpose()?,
            value.archived_at.map(u64_to_i64).transpose()?,
            u64_to_i64(value.created_at)?,
            u64_to_i64(value.updated_at)?,
        ],
    )
    .await?;
    Ok(())
}

pub(super) async fn load_penpot_designs_async(
    conn: &Connection,
    project_id: Option<ProjectId>,
) -> Result<Vec<StoredPenpotDesign>> {
    let sql = "SELECT id, project_id, connection_id, penpot_file_id, penpot_project_id,
                      penpot_team_id, name, page_id, source_doc, source_task_json,
                      last_synced_at, archived_at, created_at, updated_at
               FROM penpot_designs
               WHERE archived_at IS NULL AND (?1 IS NULL OR project_id = ?1)
               ORDER BY updated_at DESC";
    let project = project_id.map(|id| id.0.to_string());
    let mut rows = conn.query(sql, [project]).await?;
    let mut values = Vec::new();
    while let Some(row) = rows.next().await? {
        values.push(penpot_design_from_row(&row)?);
    }
    Ok(values)
}

fn penpot_design_from_row(row: &turso::Row) -> Result<StoredPenpotDesign> {
    Ok(StoredPenpotDesign {
        id: parse_uuid(&row.get::<String>(0)?)?,
        project_id: ProjectId(parse_uuid(&row.get::<String>(1)?)?),
        connection_id: parse_uuid(&row.get::<String>(2)?)?,
        penpot_file_id: parse_uuid(&row.get::<String>(3)?)?,
        penpot_project_id: parse_uuid(&row.get::<String>(4)?)?,
        penpot_team_id: parse_uuid(&row.get::<String>(5)?)?,
        name: row.get(6)?,
        page_id: opt_text(row, 7)?.map(|v| parse_uuid(&v)).transpose()?,
        source_doc: opt_text(row, 8)?.map(PathBuf::from),
        source_task: opt_text(row, 9)?
            .map(|value| serde_json::from_str(&value))
            .transpose()?,
        last_synced_at: opt_i64(row, 10)?.map(i64_to_u64).transpose()?,
        archived_at: opt_i64(row, 11)?.map(i64_to_u64).transpose()?,
        created_at: i64_to_u64(row.get(12)?)?,
        updated_at: i64_to_u64(row.get(13)?)?,
    })
}

#[cfg(test)]
async fn create_penpot_conversation_async(
    conn: &Connection,
    design_id: Uuid,
    inherited: Option<&StoredPenpotDesignConversation>,
) -> Result<StoredPenpotDesignConversation> {
    let previous = match inherited {
        Some(value) => Some(value.clone()),
        None => load_current_penpot_conversation_async(conn, design_id).await?,
    };
    let mut rows = conn
        .query(
            "SELECT COALESCE(MAX(ordinal), 0) + 1 FROM penpot_design_conversations
             WHERE design_id = ?1",
            [design_id.to_string()],
        )
        .await?;
    let ordinal: i64 = rows
        .next()
        .await?
        .map(|row| row.get(0))
        .transpose()?
        .unwrap_or(1);
    drop(rows);
    let now = unix_now();
    let conversation = StoredPenpotDesignConversation {
        id: Uuid::new_v4(),
        design_id,
        agent_id: Uuid::new_v4(),
        ordinal,
        title: format!("Conversation {ordinal}"),
        provider: previous
            .as_ref()
            .map(|v| v.provider)
            .unwrap_or(AgentKind::Codex),
        model: previous
            .as_ref()
            .map(|v| v.model.clone())
            .unwrap_or_else(|| AgentModel::default_for(AgentKind::Codex)),
        external_model_id: previous
            .as_ref()
            .and_then(|value| value.external_model_id.clone()),
        external_model_label: previous
            .as_ref()
            .and_then(|value| value.external_model_label.clone()),
        external_model_variants: previous
            .as_ref()
            .map(|value| value.external_model_variants.clone())
            .unwrap_or_default(),
        effort: previous
            .as_ref()
            .map(|v| v.effort)
            .unwrap_or(AgentEffort::Medium),
        access_mode: previous
            .as_ref()
            .map(|v| v.access_mode)
            .unwrap_or(AgentAccessMode::FullAccess),
        chat_session_id: None,
        cli_session_id: None,
        last_transcript_path: None,
        is_current: true,
        created_at: now,
        updated_at: now,
        deleted_at: None,
    };
    let persisted = conversation.clone();
    execute_transaction(conn, |conn| {
        Box::pin(async move {
            conn.execute(
                "UPDATE penpot_design_conversations SET is_current = 0 WHERE design_id = ?1",
                [design_id.to_string()],
            )
            .await?;
            insert_penpot_conversation_async(conn, &persisted).await
        })
    })
    .await?;
    Ok(conversation)
}

pub(super) async fn insert_penpot_conversation_async(
    conn: &Connection,
    value: &StoredPenpotDesignConversation,
) -> Result<()> {
    conn.execute(
        "INSERT INTO penpot_design_conversations
         (id, design_id, agent_id, ordinal, title, provider, model, external_model_id,
          external_model_label, external_model_variants, effort, access_mode,
          chat_session_id, cli_session_id, last_transcript_path, is_current,
          created_at, updated_at, deleted_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                 ?15, ?16, ?17, ?18, ?19)
         ON CONFLICT(id) DO UPDATE SET
           title = excluded.title,
           provider = excluded.provider,
           model = excluded.model,
           external_model_id = excluded.external_model_id,
           external_model_label = excluded.external_model_label,
           external_model_variants = excluded.external_model_variants,
           effort = excluded.effort,
           access_mode = excluded.access_mode,
           chat_session_id = excluded.chat_session_id,
           cli_session_id = excluded.cli_session_id,
           last_transcript_path = excluded.last_transcript_path,
           is_current = excluded.is_current,
           updated_at = excluded.updated_at,
           deleted_at = excluded.deleted_at",
        params![
            value.id.to_string(),
            value.design_id.to_string(),
            value.agent_id.to_string(),
            value.ordinal,
            value.title.as_str(),
            serde_label(&value.provider)?,
            serde_label(&value.model)?,
            value.external_model_id.as_deref(),
            value.external_model_label.as_deref(),
            serde_json::to_string(&value.external_model_variants)?,
            serde_label(&value.effort)?,
            serde_label(&value.access_mode)?,
            value.chat_session_id.as_deref(),
            value.cli_session_id.as_deref(),
            opt_path_to_string(value.last_transcript_path.as_deref()),
            bool_to_i64(value.is_current),
            u64_to_i64(value.created_at)?,
            u64_to_i64(value.updated_at)?,
            value.deleted_at.map(u64_to_i64).transpose()?,
        ],
    )
    .await?;
    Ok(())
}

#[cfg(test)]
pub(super) async fn load_current_penpot_conversation_async(
    conn: &Connection,
    design_id: Uuid,
) -> Result<Option<StoredPenpotDesignConversation>> {
    let values = load_penpot_conversations_async(conn, design_id).await?;
    Ok(values.into_iter().find(|value| value.is_current))
}

#[cfg(test)]
pub(super) async fn load_penpot_conversations_async(
    conn: &Connection,
    design_id: Uuid,
) -> Result<Vec<StoredPenpotDesignConversation>> {
    let mut rows = conn
        .query(
            "SELECT id, design_id, agent_id, ordinal, title, provider, model,
                    external_model_id, external_model_label, external_model_variants, effort,
                    access_mode, chat_session_id, cli_session_id, last_transcript_path,
                    is_current, created_at, updated_at, deleted_at
             FROM penpot_design_conversations
             WHERE design_id = ?1 AND deleted_at IS NULL
             ORDER BY ordinal DESC",
            [design_id.to_string()],
        )
        .await?;
    let mut values = Vec::new();
    while let Some(row) = rows.next().await? {
        values.push(StoredPenpotDesignConversation {
            id: parse_uuid(&row.get::<String>(0)?)?,
            design_id: parse_uuid(&row.get::<String>(1)?)?,
            agent_id: parse_uuid(&row.get::<String>(2)?)?,
            ordinal: row.get(3)?,
            title: row.get(4)?,
            provider: serde_parse(&row.get::<String>(5)?)?,
            model: serde_parse(&row.get::<String>(6)?)?,
            external_model_id: opt_text(&row, 7)?,
            external_model_label: opt_text(&row, 8)?,
            external_model_variants: serde_json::from_str(&row.get::<String>(9)?)?,
            effort: serde_parse(&row.get::<String>(10)?)?,
            access_mode: serde_parse(&row.get::<String>(11)?)?,
            chat_session_id: opt_text(&row, 12)?,
            cli_session_id: opt_text(&row, 13)?,
            last_transcript_path: opt_text(&row, 14)?.map(PathBuf::from),
            is_current: row.get::<i64>(15)? != 0,
            created_at: i64_to_u64(row.get(16)?)?,
            updated_at: i64_to_u64(row.get(17)?)?,
            deleted_at: opt_i64(&row, 18)?.map(i64_to_u64).transpose()?,
        });
    }
    Ok(values)
}

pub(super) async fn load_all_penpot_conversations_async(
    conn: &Connection,
) -> Result<Vec<StoredPenpotDesignConversation>> {
    let mut rows = conn
        .query(
            "SELECT id, design_id, agent_id, ordinal, title, provider, model,
                    external_model_id, external_model_label, external_model_variants, effort,
                    access_mode, chat_session_id, cli_session_id, last_transcript_path,
                    is_current, created_at, updated_at, deleted_at
             FROM penpot_design_conversations ORDER BY design_id ASC, ordinal ASC",
            (),
        )
        .await?;
    let mut values = Vec::new();
    while let Some(row) = rows.next().await? {
        values.push(StoredPenpotDesignConversation {
            id: parse_uuid(&row.get::<String>(0)?)?,
            design_id: parse_uuid(&row.get::<String>(1)?)?,
            agent_id: parse_uuid(&row.get::<String>(2)?)?,
            ordinal: row.get(3)?,
            title: row.get(4)?,
            provider: serde_parse(&row.get::<String>(5)?)?,
            model: serde_parse(&row.get::<String>(6)?)?,
            external_model_id: opt_text(&row, 7)?,
            external_model_label: opt_text(&row, 8)?,
            external_model_variants: serde_json::from_str(&row.get::<String>(9)?)?,
            effort: serde_parse(&row.get::<String>(10)?)?,
            access_mode: serde_parse(&row.get::<String>(11)?)?,
            chat_session_id: opt_text(&row, 12)?,
            cli_session_id: opt_text(&row, 13)?,
            last_transcript_path: opt_text(&row, 14)?.map(PathBuf::from),
            is_current: row.get::<i64>(15)? != 0,
            created_at: i64_to_u64(row.get(16)?)?,
            updated_at: i64_to_u64(row.get(17)?)?,
            deleted_at: opt_i64(&row, 18)?.map(i64_to_u64).transpose()?,
        });
    }
    Ok(values)
}
