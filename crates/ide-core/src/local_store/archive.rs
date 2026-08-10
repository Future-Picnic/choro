use super::*;

pub(super) async fn clear_imported_tables(conn: &Connection) -> Result<()> {
    for table in [
        "penpot_design_conversations",
        "project_penpot_bindings",
        "penpot_designs",
        "penpot_connections",
        "agent_diff_files",
        "agent_diff_snapshots",
        "attachments",
        "agent_messages",
        "agent_summaries",
        "agent_search_fts",
        "chat_messages_fts",
        "memories",
        "chat_timeline_events",
        "chat_messages",
        "agent_runtime_sessions",
        "agent_changed_files",
        "agent_linked_tasks",
        "agent_linked_docs",
        "agents",
        "personal_tasks",
        "project_references",
        "project_git_workflow_runs",
        "project_git_workflows",
        "project_task_tracker_connections",
        "project_db_connections",
        "project_presets",
        "projects",
        "project_sections",
    ] {
        conn.execute(format!("DELETE FROM {table}"), ()).await?;
    }
    Ok(())
}

pub(super) async fn delete_missing(
    conn: &Connection,
    table: &str,
    ids: &HashSet<String>,
) -> Result<()> {
    let mut rows = conn.query(format!("SELECT id FROM {table}"), ()).await?;
    let mut delete = Vec::new();
    while let Some(row) = rows.next().await? {
        let id: String = row.get(0)?;
        if !ids.contains(&id) {
            delete.push(id);
        }
    }
    drop(rows);
    for id in delete {
        conn.execute(format!("DELETE FROM {table} WHERE id = ?1"), [id])
            .await?;
    }
    Ok(())
}

pub(super) fn write_zip_json<T: Serialize>(
    zip: &mut ZipWriter<File>,
    options: FileOptions,
    name: &str,
    value: &T,
    checksums: &mut Vec<ExportChecksum>,
) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    checksums.push(ExportChecksum {
        path: name.to_string(),
        sha256: sha256_hex(&bytes),
    });
    zip.start_file(name, options)?;
    zip.write_all(&bytes)?;
    Ok(())
}

pub(super) fn write_zip_jsonl<T: Serialize>(
    zip: &mut ZipWriter<File>,
    options: FileOptions,
    name: &str,
    values: &[T],
    checksums: &mut Vec<ExportChecksum>,
) -> Result<()> {
    let mut bytes = Vec::new();
    for value in values {
        serde_json::to_writer(&mut bytes, value)?;
        bytes.push(b'\n');
    }
    checksums.push(ExportChecksum {
        path: name.to_string(),
        sha256: sha256_hex(&bytes),
    });
    zip.start_file(name, options)?;
    zip.write_all(&bytes)?;
    Ok(())
}

pub(super) fn read_zip_json<T: DeserializeOwned>(
    zip: &mut ZipArchive<File>,
    name: &str,
) -> Result<T> {
    let mut file = zip.by_name(name)?;
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    Ok(serde_json::from_str(&text)?)
}

pub(super) fn read_zip_jsonl<T: DeserializeOwned>(
    zip: &mut ZipArchive<File>,
    name: &str,
) -> Result<Vec<T>> {
    let file = zip.by_name(name)?;
    let reader = BufReader::new(file);
    reader
        .lines()
        .map_while(|line| line.ok())
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str::<T>(&line).map_err(Into::into))
        .collect()
}

pub(super) fn read_zip_jsonl_optional<T: DeserializeOwned>(
    zip: &mut ZipArchive<File>,
    name: &str,
) -> Result<Vec<T>> {
    match zip.by_name(name) {
        Ok(file) => {
            let reader = BufReader::new(file);
            reader
                .lines()
                .map_while(|line| line.ok())
                .filter(|line| !line.trim().is_empty())
                .map(|line| serde_json::from_str::<T>(&line).map_err(Into::into))
                .collect()
        }
        Err(zip::result::ZipError::FileNotFound) => Ok(Vec::new()),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn copy_dir_recursive(source: &Path, target: &Path) -> Result<()> {
    fs::create_dir_all(target)
        .with_context(|| format!("failed to create directory {}", target.display()))?;
    for entry in fs::read_dir(source)
        .with_context(|| format!("failed to read directory {}", source.display()))?
    {
        let entry = entry?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        if source_path.is_dir() {
            copy_dir_recursive(&source_path, &target_path)?;
        } else {
            fs::copy(&source_path, &target_path).with_context(|| {
                format!(
                    "failed to copy {} to {}",
                    source_path.display(),
                    target_path.display()
                )
            })?;
        }
    }
    Ok(())
}
