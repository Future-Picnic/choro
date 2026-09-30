use super::*;

pub(super) async fn upsert_project_preview_async(
    conn: &Connection,
    project_id: ProjectId,
    url: &str,
    title: &str,
    source_agent_id: Option<Uuid>,
) -> Result<StoredProjectPreview> {
    let url = url.trim();
    let scheme = url::Url::parse(url)
        .map(|url| url.scheme().to_string())
        .map_err(|_| anyhow!("project preview target must be a valid URL"))?;
    if !matches!(scheme.as_str(), "http" | "https" | "file") {
        return Err(anyhow!("project preview URL must use HTTP, HTTPS, or FILE"));
    }

    let project_exists = {
        let mut rows = conn
            .query(
                "SELECT 1 FROM projects WHERE id = ?1",
                [project_id.0.to_string()],
            )
            .await?;
        rows.next().await?.is_some()
    };
    if !project_exists {
        return Err(anyhow!("cannot open a preview for an unknown project"));
    }
    if let Some(agent_id) = source_agent_id {
        let mut rows = conn
            .query(
                "SELECT project_id FROM agents WHERE id = ?1",
                [agent_id.to_string()],
            )
            .await?;
        let Some(row) = rows.next().await? else {
            return Err(anyhow!("preview source agent is not stored in Choro"));
        };
        if ProjectId(parse_uuid(&row.get::<String>(0)?)?) != project_id {
            return Err(anyhow!("preview source agent belongs to another project"));
        }
    }

    let existing_id = {
        let mut rows = conn
            .query(
                "SELECT id FROM project_previews WHERE project_id = ?1 AND url = ?2",
                (project_id.0.to_string(), url),
            )
            .await?;
        match rows.next().await? {
            Some(row) => Some(parse_uuid(&row.get::<String>(0)?)?),
            None => None,
        }
    };
    let id = existing_id.unwrap_or_else(Uuid::new_v4);
    let now = unix_now();
    conn.execute(
        "INSERT INTO project_previews
         (id, project_id, url, title, source_agent_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(project_id, url) DO UPDATE SET
             title = CASE WHEN excluded.title = '' THEN project_previews.title ELSE excluded.title END,
             source_agent_id = excluded.source_agent_id,
             updated_at = CASE
                 WHEN project_previews.updated_at >= excluded.updated_at
                 THEN project_previews.updated_at + 1
                 ELSE excluded.updated_at
             END",
        params![
            id.to_string(),
            project_id.0.to_string(),
            url,
            title.trim(),
            source_agent_id.map(|value| value.to_string()),
            u64_to_i64(now)?,
            u64_to_i64(now)?,
        ],
    )
    .await?;

    load_project_preview_async(conn, id)
        .await?
        .ok_or_else(|| anyhow!("failed to load stored project preview"))
}

async fn load_project_preview_async(
    conn: &Connection,
    id: Uuid,
) -> Result<Option<StoredProjectPreview>> {
    let mut rows = conn
        .query(
            "SELECT id, project_id, url, title, source_agent_id, created_at, updated_at
             FROM project_previews WHERE id = ?1",
            [id.to_string()],
        )
        .await?;
    let Some(row) = rows.next().await? else {
        return Ok(None);
    };
    Ok(Some(project_preview_from_row(&row)?))
}

pub(super) async fn load_project_previews_async(
    conn: &Connection,
    project_id: ProjectId,
) -> Result<Vec<StoredProjectPreview>> {
    let mut rows = conn
        .query(
            "SELECT id, project_id, url, title, source_agent_id, created_at, updated_at
             FROM project_previews WHERE project_id = ?1 ORDER BY updated_at DESC",
            [project_id.0.to_string()],
        )
        .await?;
    let mut previews = Vec::new();
    while let Some(row) = rows.next().await? {
        previews.push(project_preview_from_row(&row)?);
    }
    Ok(previews)
}

pub(super) async fn clear_project_previews_async(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM project_previews", ()).await?;
    Ok(())
}

fn project_preview_from_row(row: &turso::Row) -> Result<StoredProjectPreview> {
    Ok(StoredProjectPreview {
        id: parse_uuid(&row.get::<String>(0)?)?,
        project_id: ProjectId(parse_uuid(&row.get::<String>(1)?)?),
        url: row.get(2)?,
        title: row.get(3)?,
        source_agent_id: opt_text(row, 4)?
            .map(|value| parse_uuid(&value))
            .transpose()?,
        created_at: i64_to_u64(row.get(5)?)?,
        updated_at: i64_to_u64(row.get(6)?)?,
    })
}
