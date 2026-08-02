use super::*;

pub(super) async fn insert_personal_task_async(
    conn: &Connection,
    task: &PersonalTaskRecord,
) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO personal_tasks
        (id, project_id, key_number, title, description_markdown, status, priority,
         labels_json, created_at, updated_at, archived)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            task.id.to_string(),
            task.project_id.0.to_string(),
            task.key_number,
            task.title.as_str(),
            task.description_markdown.as_str(),
            serde_label(&task.status)?,
            serde_label(&task.priority)?,
            serde_json::to_string(&task.labels)?,
            u64_to_i64(task.created_at)?,
            u64_to_i64(task.updated_at)?,
            bool_to_i64(task.archived),
        ],
    )
    .await?;
    Ok(())
}

pub(super) async fn load_personal_tasks_async(
    conn: &Connection,
    project_id: Option<ProjectId>,
) -> Result<Vec<PersonalTaskRecord>> {
    let sql = "SELECT id, project_id, key_number, title, description_markdown, status, priority,
               labels_json, created_at, updated_at, archived
               FROM personal_tasks";
    let mut rows = if let Some(project_id) = project_id {
        conn.query(
            format!("{sql} WHERE project_id = ?1 ORDER BY archived ASC, key_number ASC"),
            [project_id.0.to_string()],
        )
        .await?
    } else {
        conn.query(
            format!("{sql} ORDER BY project_id ASC, archived ASC, key_number ASC"),
            (),
        )
        .await?
    };
    let mut tasks = Vec::new();
    while let Some(row) = rows.next().await? {
        let labels_json: String = row.get(7)?;
        tasks.push(PersonalTaskRecord {
            id: parse_uuid(&row.get::<String>(0)?)?,
            project_id: ProjectId(parse_uuid(&row.get::<String>(1)?)?),
            key_number: row.get(2)?,
            title: row.get(3)?,
            description_markdown: row.get(4)?,
            status: serde_parse(&row.get::<String>(5)?)?,
            priority: serde_parse(&row.get::<String>(6)?)?,
            labels: serde_json::from_str(&labels_json).unwrap_or_default(),
            created_at: i64_to_u64(row.get(8)?)?,
            updated_at: i64_to_u64(row.get(9)?)?,
            archived: row.get::<i64>(10)? != 0,
        });
    }
    Ok(tasks)
}

pub(super) async fn next_personal_task_key_number(
    conn: &Connection,
    project_id: ProjectId,
) -> Result<i64> {
    let mut rows = conn
        .query(
            "SELECT COALESCE(MAX(key_number), 0) + 1 FROM personal_tasks WHERE project_id = ?1",
            [project_id.0.to_string()],
        )
        .await?;
    let Some(row) = rows.next().await? else {
        return Ok(1);
    };
    Ok(row.get(0)?)
}
