use super::*;

pub(super) async fn save_project_sections_async(
    conn: &Connection,
    sections: &[ProjectSection],
) -> Result<()> {
    let ids: HashSet<String> = sections
        .iter()
        .map(|section| section.id.0.to_string())
        .collect();
    delete_missing(conn, "project_sections", &ids).await?;
    for section in sections {
        conn.execute(
            "INSERT OR REPLACE INTO project_sections (id, name, collapsed) VALUES (?1, ?2, ?3)",
            (
                section.id.0.to_string(),
                section.name.as_str(),
                bool_to_i64(section.collapsed),
            ),
        )
        .await?;
    }
    Ok(())
}

pub(super) async fn save_projects_async(conn: &Connection, projects: &[Project]) -> Result<()> {
    let ids: HashSet<String> = projects
        .iter()
        .map(|project| project.id.0.to_string())
        .collect();
    delete_missing(conn, "projects", &ids).await?;
    for project in projects {
        conn.execute(
            "INSERT INTO projects
            (id, name, path, icon, icon_color, icon_image_path, section_id, is_favorite)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                path = excluded.path,
                icon = excluded.icon,
                icon_color = excluded.icon_color,
                icon_image_path = excluded.icon_image_path,
                section_id = excluded.section_id,
                is_favorite = excluded.is_favorite",
            params![
                project.id.0.to_string(),
                project.name.as_str(),
                path_to_string(&project.path),
                project.icon.as_str(),
                project.icon_color.as_str(),
                opt_path_to_string(project.icon_image_path.as_deref()),
                project.section_id.map(|id| id.0.to_string()),
                bool_to_i64(project.is_favorite),
            ],
        )
        .await?;
        conn.execute(
            "DELETE FROM project_presets WHERE project_id = ?1",
            [project.id.0.to_string()],
        )
        .await?;
        for (index, preset) in project.presets.iter().enumerate() {
            conn.execute(
                "INSERT INTO project_presets (id, project_id, name, command, sort_order)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                (
                    preset.id.to_string(),
                    project.id.0.to_string(),
                    preset.name.as_str(),
                    preset.command.as_str(),
                    index as i64,
                ),
            )
            .await?;
        }
        conn.execute(
            "DELETE FROM project_db_connections WHERE project_id = ?1",
            [project.id.0.to_string()],
        )
        .await?;
        for (index, db) in project.db_connections.iter().enumerate() {
            conn.execute(
                "INSERT INTO project_db_connections
                 (id, project_id, provider, read_only, name, uri, sort_order)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                (
                    db.id.to_string(),
                    project.id.0.to_string(),
                    db.provider.as_str(),
                    bool_to_i64(db.read_only),
                    db.name.as_str(),
                    db.uri.as_str(),
                    index as i64,
                ),
            )
            .await?;
        }
        conn.execute(
            "DELETE FROM project_task_tracker_connections WHERE project_id = ?1",
            [project.id.0.to_string()],
        )
        .await?;
        for (index, tracker) in project.task_tracker_connections.iter().enumerate() {
            conn.execute(
                "INSERT INTO project_task_tracker_connections
                 (id, project_id, provider, name, site_url, email, api_token, board_id,
                  board_name, assignee_filter, assignee_account_id, assignee_display_name,
                  source_id, source_name, source_kind, provider_config_json, filters_json,
                  sort_order)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
                params![
                    tracker.id.to_string(),
                    project.id.0.to_string(),
                    serde_label(&tracker.provider)?,
                    tracker.name.as_str(),
                    tracker.site_url.as_str(),
                    tracker.email.as_str(),
                    tracker.api_token.as_str(),
                    tracker.board_id,
                    tracker.board_name.clone(),
                    tracker.assignee_filter.clone(),
                    tracker.assignee_account_id.clone(),
                    tracker.assignee_display_name.clone(),
                    tracker.source_id.clone(),
                    tracker.source_name.clone(),
                    tracker.source_kind.clone(),
                    tracker.provider_config_json.as_str(),
                    tracker.filters_json.as_str(),
                    index as i64,
                ],
            )
            .await?;
        }
    }
    Ok(())
}

pub(super) async fn load_project_sections_async(conn: &Connection) -> Result<Vec<ProjectSection>> {
    let mut rows = conn
        .query(
            "SELECT id, name, collapsed FROM project_sections ORDER BY name ASC",
            (),
        )
        .await?;
    let mut sections = Vec::new();
    while let Some(row) = rows.next().await? {
        sections.push(ProjectSection {
            id: ProjectSectionId(parse_uuid(&row.get::<String>(0)?)?),
            name: row.get(1)?,
            collapsed: row.get::<i64>(2)? != 0,
        });
    }
    Ok(sections)
}

pub(super) async fn load_projects_async(conn: &Connection) -> Result<Vec<Project>> {
    let presets = load_project_presets(conn).await?;
    let db_connections = load_project_db_connections(conn).await?;
    let task_tracker_connections = load_project_task_tracker_connections(conn).await?;
    let mut rows = conn
        .query(
            "SELECT id, name, path, icon, icon_color, icon_image_path, section_id, is_favorite
             FROM projects ORDER BY name ASC",
            (),
        )
        .await?;
    let mut projects = Vec::new();
    while let Some(row) = rows.next().await? {
        let id = ProjectId(parse_uuid(&row.get::<String>(0)?)?);
        let section_id = opt_text(&row, 6)?
            .map(|id| parse_uuid(&id).map(ProjectSectionId))
            .transpose()?;
        projects.push(Project {
            id,
            name: row.get(1)?,
            path: PathBuf::from(row.get::<String>(2)?),
            icon: row.get(3)?,
            icon_color: row.get(4)?,
            icon_image_path: opt_text(&row, 5)?.map(PathBuf::from),
            section_id,
            is_favorite: row.get::<i64>(7)? != 0,
            presets: presets.get(&id).cloned().unwrap_or_default(),
            db_connections: db_connections.get(&id).cloned().unwrap_or_default(),
            task_tracker_connections: task_tracker_connections
                .get(&id)
                .cloned()
                .unwrap_or_default(),
        });
    }
    Ok(projects)
}

pub(super) async fn load_project_presets(
    conn: &Connection,
) -> Result<HashMap<ProjectId, Vec<ScriptPreset>>> {
    let mut rows = conn
        .query(
            "SELECT id, project_id, name, command FROM project_presets
             ORDER BY project_id ASC, sort_order ASC",
            (),
        )
        .await?;
    let mut presets: HashMap<ProjectId, Vec<ScriptPreset>> = HashMap::new();
    while let Some(row) = rows.next().await? {
        let project_id = ProjectId(parse_uuid(&row.get::<String>(1)?)?);
        presets.entry(project_id).or_default().push(ScriptPreset {
            id: parse_uuid(&row.get::<String>(0)?)?,
            name: row.get(2)?,
            command: row.get(3)?,
        });
    }
    Ok(presets)
}

pub(super) async fn load_project_db_connections(
    conn: &Connection,
) -> Result<HashMap<ProjectId, Vec<DbConnection>>> {
    let mut rows = conn
        .query(
            "SELECT id, project_id, provider, read_only, name, uri FROM project_db_connections
             ORDER BY project_id ASC, sort_order ASC",
            (),
        )
        .await?;
    let mut connections: HashMap<ProjectId, Vec<DbConnection>> = HashMap::new();
    while let Some(row) = rows.next().await? {
        let project_id = ProjectId(parse_uuid(&row.get::<String>(1)?)?);
        connections
            .entry(project_id)
            .or_default()
            .push(DbConnection {
                id: parse_uuid(&row.get::<String>(0)?)?,
                provider: row.get::<String>(2)?.parse()?,
                read_only: row.get::<i64>(3)? != 0,
                name: row.get(4)?,
                uri: row.get(5)?,
            });
    }
    Ok(connections)
}

pub(super) async fn load_project_task_tracker_connections(
    conn: &Connection,
) -> Result<HashMap<ProjectId, Vec<TaskTrackerConnection>>> {
    let mut rows = conn
        .query(
            "SELECT id, project_id, provider, name, site_url, email, api_token, board_id,
             board_name, assignee_filter, assignee_account_id, assignee_display_name,
             source_id, source_name, source_kind, provider_config_json, filters_json
             FROM project_task_tracker_connections
             ORDER BY project_id ASC, sort_order ASC",
            (),
        )
        .await?;
    let mut connections: HashMap<ProjectId, Vec<TaskTrackerConnection>> = HashMap::new();
    while let Some(row) = rows.next().await? {
        let project_id = ProjectId(parse_uuid(&row.get::<String>(1)?)?);
        connections
            .entry(project_id)
            .or_default()
            .push(TaskTrackerConnection {
                id: parse_uuid(&row.get::<String>(0)?)?,
                provider: serde_parse(&row.get::<String>(2)?)?,
                name: row.get(3)?,
                site_url: row.get(4)?,
                email: row.get(5)?,
                api_token: row.get(6)?,
                source_id: opt_text(&row, 12)?,
                source_name: opt_text(&row, 13)?,
                source_kind: opt_text(&row, 14)?,
                provider_config_json: opt_text(&row, 15)?.unwrap_or_else(|| "{}".to_string()),
                filters_json: opt_text(&row, 16)?.unwrap_or_else(|| "{}".to_string()),
                board_id: opt_i64(&row, 7)?,
                board_name: opt_text(&row, 8)?,
                assignee_filter: opt_text(&row, 9)?,
                assignee_account_id: opt_text(&row, 10)?,
                assignee_display_name: opt_text(&row, 11)?,
            });
    }
    Ok(connections)
}

pub(super) async fn insert_project_reference_async(
    conn: &Connection,
    reference: &ProjectReference,
) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO project_references
        (id, project_id, kind, title, source, preview_relative_path, notes, metadata_json,
         sort_order, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            reference.id.to_string(),
            reference.project_id.0.to_string(),
            reference.kind.as_str(),
            reference.title.as_str(),
            reference.source.as_str(),
            opt_path_to_string(reference.preview_relative_path.as_deref()),
            reference.notes.as_str(),
            reference.metadata_json.as_str(),
            reference.sort_order,
            u64_to_i64(reference.created_at)?,
            u64_to_i64(reference.updated_at)?,
        ],
    )
    .await?;
    Ok(())
}

pub(super) async fn load_project_references_async(
    conn: &Connection,
    project_id: ProjectId,
) -> Result<Vec<ProjectReference>> {
    let mut rows = conn
        .query(
            "SELECT id, project_id, kind, title, source, preview_relative_path, notes,
             metadata_json, sort_order, created_at, updated_at
             FROM project_references WHERE project_id = ?1
             ORDER BY sort_order ASC, created_at ASC",
            [project_id.0.to_string()],
        )
        .await?;
    project_references_from_rows(&mut rows).await
}

pub(super) async fn load_all_project_references_async(
    conn: &Connection,
) -> Result<Vec<ProjectReference>> {
    let mut rows = conn
        .query(
            "SELECT id, project_id, kind, title, source, preview_relative_path, notes,
             metadata_json, sort_order, created_at, updated_at
             FROM project_references ORDER BY project_id ASC, sort_order ASC, created_at ASC",
            (),
        )
        .await?;
    project_references_from_rows(&mut rows).await
}

pub(super) async fn project_references_from_rows(
    rows: &mut turso::Rows,
) -> Result<Vec<ProjectReference>> {
    let mut references = Vec::new();
    while let Some(row) = rows.next().await? {
        references.push(project_reference_from_row(&row)?);
    }
    Ok(references)
}

pub(super) fn project_reference_from_row(row: &turso::Row) -> Result<ProjectReference> {
    let kind_label: String = row.get(2)?;
    let kind = ProjectReferenceKind::from_label(&kind_label)
        .ok_or_else(|| anyhow!("unknown project reference kind {kind_label}"))?;
    Ok(ProjectReference {
        id: parse_uuid(&row.get::<String>(0)?)?,
        project_id: ProjectId(parse_uuid(&row.get::<String>(1)?)?),
        kind,
        title: row.get(3)?,
        source: row.get(4)?,
        preview_relative_path: opt_text(row, 5)?.map(PathBuf::from),
        notes: row.get(6)?,
        metadata_json: row.get(7)?,
        sort_order: row.get(8)?,
        created_at: i64_to_u64(row.get(9)?)?,
        updated_at: i64_to_u64(row.get(10)?)?,
    })
}

pub(super) async fn next_project_reference_sort_order(
    conn: &Connection,
    project_id: ProjectId,
) -> Result<i64> {
    let mut rows = conn
        .query(
            "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM project_references WHERE project_id = ?1",
            [project_id.0.to_string()],
        )
        .await?;
    let Some(row) = rows.next().await? else {
        return Ok(0);
    };
    Ok(row.get(0)?)
}
