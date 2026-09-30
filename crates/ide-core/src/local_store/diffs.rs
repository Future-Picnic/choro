use super::*;

pub(super) async fn save_agent_diff_snapshot_async(
    conn: &Connection,
    snapshot: &StoredAgentDiffSnapshot,
) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO agent_diff_snapshots
        (id, agent_id, project_id, repo_path, source, base_sha, head_sha, commit_sha,
         created_at, updated_at, state)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            snapshot.id.to_string(),
            snapshot.agent_id.to_string(),
            snapshot.project_id.0.to_string(),
            path_to_string(&snapshot.repo_path),
            snapshot.source.as_str(),
            snapshot.base_sha.clone(),
            snapshot.head_sha.clone(),
            snapshot.commit_sha.clone(),
            u64_to_i64(snapshot.created_at)?,
            u64_to_i64(snapshot.updated_at)?,
            snapshot.state.as_str(),
        ],
    )
    .await?;
    conn.execute(
        "DELETE FROM agent_diff_files WHERE snapshot_id = ?1",
        [snapshot.id.to_string()],
    )
    .await?;
    for file in &snapshot.files {
        insert_agent_diff_file_async(conn, file).await?;
    }
    Ok(())
}

pub(super) async fn insert_agent_diff_file_async(
    conn: &Connection,
    file: &StoredAgentDiffFile,
) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO agent_diff_files
        (snapshot_id, path, additions, deletions, is_binary, diff_json, truncated, sort_order)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            file.snapshot_id.to_string(),
            path_to_string(&file.path),
            file.additions as i64,
            file.deletions as i64,
            bool_to_i64(file.is_binary),
            serde_json::to_string(&file.diff)?,
            bool_to_i64(file.truncated),
            file.sort_order,
        ],
    )
    .await?;
    Ok(())
}

pub(super) async fn load_agent_diff_snapshot_async(
    conn: &Connection,
    snapshot_id: Uuid,
) -> Result<Option<StoredAgentDiffSnapshot>> {
    let mut rows = conn
        .query(
            "SELECT id, agent_id, project_id, repo_path, source, base_sha, head_sha, commit_sha,
             created_at, updated_at, state FROM agent_diff_snapshots WHERE id = ?1",
            [snapshot_id.to_string()],
        )
        .await?;
    let Some(row) = rows.next().await? else {
        return Ok(None);
    };
    let row = diff_snapshot_row_from_row(&row)?;
    drop(rows);
    let files = load_agent_diff_files_async(conn, snapshot_id).await?;
    Ok(Some(row.into_snapshot(files)))
}

pub(super) async fn load_all_agent_diff_snapshots_async(
    conn: &Connection,
) -> Result<Vec<StoredAgentDiffSnapshot>> {
    let mut rows = conn
        .query(
            "SELECT id, agent_id, project_id, repo_path, source, base_sha, head_sha, commit_sha,
             created_at, updated_at, state FROM agent_diff_snapshots ORDER BY created_at ASC",
            (),
        )
        .await?;
    let mut snapshot_rows = Vec::new();
    while let Some(row) = rows.next().await? {
        snapshot_rows.push(diff_snapshot_row_from_row(&row)?);
    }
    drop(rows);
    let all_files = load_all_agent_diff_files_async(conn).await?;
    Ok(inflate_diff_snapshots(snapshot_rows, all_files))
}

pub(super) async fn load_agent_diff_files_async(
    conn: &Connection,
    snapshot_id: Uuid,
) -> Result<Vec<StoredAgentDiffFile>> {
    let mut rows = conn
        .query(
            "SELECT snapshot_id, path, additions, deletions, is_binary, diff_json, truncated,
             sort_order FROM agent_diff_files WHERE snapshot_id = ?1 ORDER BY sort_order ASC",
            [snapshot_id.to_string()],
        )
        .await?;
    diff_files_from_rows(&mut rows).await
}

pub(super) async fn load_all_agent_diff_files_async(
    conn: &Connection,
) -> Result<Vec<StoredAgentDiffFile>> {
    let mut rows = conn
        .query(
            "SELECT snapshot_id, path, additions, deletions, is_binary, diff_json, truncated,
             sort_order FROM agent_diff_files ORDER BY snapshot_id ASC, sort_order ASC",
            (),
        )
        .await?;
    diff_files_from_rows(&mut rows).await
}

pub(super) async fn diff_files_from_rows(
    rows: &mut turso::Rows,
) -> Result<Vec<StoredAgentDiffFile>> {
    let mut files = Vec::new();
    while let Some(row) = rows.next().await? {
        files.push(StoredAgentDiffFile {
            snapshot_id: parse_uuid(&row.get::<String>(0)?)?,
            path: PathBuf::from(row.get::<String>(1)?),
            additions: i64_to_usize(row.get(2)?)?,
            deletions: i64_to_usize(row.get(3)?)?,
            is_binary: row.get::<i64>(4)? != 0,
            diff: serde_json::from_str(&row.get::<String>(5)?)?,
            truncated: row.get::<i64>(6)? != 0,
            sort_order: row.get(7)?,
        });
    }
    Ok(files)
}

pub(super) fn diff_snapshot_row_from_row(row: &turso::Row) -> Result<StoredAgentDiffSnapshotRow> {
    Ok(StoredAgentDiffSnapshotRow {
        id: parse_uuid(&row.get::<String>(0)?)?,
        agent_id: parse_uuid(&row.get::<String>(1)?)?,
        project_id: ProjectId(parse_uuid(&row.get::<String>(2)?)?),
        repo_path: PathBuf::from(row.get::<String>(3)?),
        source: row.get(4)?,
        base_sha: opt_text(row, 5)?,
        head_sha: opt_text(row, 6)?,
        commit_sha: opt_text(row, 7)?,
        created_at: i64_to_u64(row.get(8)?)?,
        updated_at: i64_to_u64(row.get(9)?)?,
        state: row.get(10)?,
    })
}

impl From<&StoredAgentDiffSnapshot> for StoredAgentDiffSnapshotRow {
    fn from(snapshot: &StoredAgentDiffSnapshot) -> Self {
        Self {
            id: snapshot.id,
            agent_id: snapshot.agent_id,
            project_id: snapshot.project_id,
            repo_path: snapshot.repo_path.clone(),
            source: snapshot.source.clone(),
            base_sha: snapshot.base_sha.clone(),
            head_sha: snapshot.head_sha.clone(),
            commit_sha: snapshot.commit_sha.clone(),
            created_at: snapshot.created_at,
            updated_at: snapshot.updated_at,
            state: snapshot.state.clone(),
        }
    }
}

impl StoredAgentDiffSnapshotRow {
    fn into_snapshot(self, files: Vec<StoredAgentDiffFile>) -> StoredAgentDiffSnapshot {
        StoredAgentDiffSnapshot {
            id: self.id,
            agent_id: self.agent_id,
            project_id: self.project_id,
            repo_path: self.repo_path,
            source: self.source,
            base_sha: self.base_sha,
            head_sha: self.head_sha,
            commit_sha: self.commit_sha,
            created_at: self.created_at,
            updated_at: self.updated_at,
            state: self.state,
            files,
        }
    }
}

pub(super) fn inflate_diff_snapshots(
    rows: Vec<StoredAgentDiffSnapshotRow>,
    files: Vec<StoredAgentDiffFile>,
) -> Vec<StoredAgentDiffSnapshot> {
    let mut by_snapshot: HashMap<Uuid, Vec<StoredAgentDiffFile>> = HashMap::new();
    for file in files {
        by_snapshot.entry(file.snapshot_id).or_default().push(file);
    }
    rows.into_iter()
        .map(|row| {
            let mut files = by_snapshot.remove(&row.id).unwrap_or_default();
            files.sort_by_key(|file| file.sort_order);
            row.into_snapshot(files)
        })
        .collect()
}

pub(super) struct BackfillSnapshotMatch {
    pub(super) source: &'static str,
    pub(super) base_sha: Option<String>,
    pub(super) head_sha: Option<String>,
    pub(super) commit_sha: Option<String>,
    pub(super) diffs: Vec<FileDiff>,
}

pub(super) fn backfill_snapshot_match(
    repo_path: &Path,
    files: &[BackfillFileChange],
) -> Option<BackfillSnapshotMatch> {
    if let Ok(worktree_diffs) = crate::git::worktree_diffs(repo_path) {
        if let Some(diffs) = matching_diffs_for_summary(repo_path, &worktree_diffs, files) {
            return Some(BackfillSnapshotMatch {
                source: "backfill_worktree",
                base_sha: git_head_sha(repo_path),
                head_sha: None,
                commit_sha: None,
                diffs,
            });
        }
    }

    let (commit_sha, diffs) = unique_recent_commit_match(repo_path, files)?;
    Some(BackfillSnapshotMatch {
        source: "backfill_commit",
        base_sha: None,
        head_sha: Some(commit_sha.clone()),
        commit_sha: Some(commit_sha),
        diffs,
    })
}

pub(super) fn unique_recent_commit_match(
    repo_path: &Path,
    files: &[BackfillFileChange],
) -> Option<(String, Vec<FileDiff>)> {
    let commits = crate::git::list_commits(repo_path, 50).ok()?;
    let mut matched = None;
    for commit in commits {
        let Ok(diffs) = crate::git::commit_diff(repo_path, &commit.sha) else {
            continue;
        };
        let Some(diffs) = matching_diffs_for_summary(repo_path, &diffs, files) else {
            continue;
        };
        if matched.is_some() {
            return None;
        }
        matched = Some((commit.sha, diffs));
    }
    matched
}

pub(super) fn matching_diffs_for_summary(
    repo_path: &Path,
    diffs: &[FileDiff],
    files: &[BackfillFileChange],
) -> Option<Vec<FileDiff>> {
    let mut by_path = HashMap::new();
    for diff in diffs {
        by_path.insert(normalize_diff_path(repo_path, &diff.path), diff);
    }
    let mut matched = Vec::new();
    let mut seen = HashSet::new();
    for file in files {
        let path = normalize_diff_path(repo_path, &file.path);
        if !seen.insert(path.clone()) {
            return None;
        }
        let diff = by_path.get(&path)?;
        let (additions, deletions) = file_diff_stats(diff);
        if additions != file.additions || deletions != file.deletions {
            return None;
        }
        matched.push((*diff).clone());
    }
    Some(matched)
}

pub(super) fn normalize_diff_path(repo_path: &Path, path: &Path) -> PathBuf {
    let relative = path.strip_prefix(repo_path).unwrap_or(path);
    let mut normalized = PathBuf::new();
    for component in relative.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::Normal(part) => normalized.push(part),
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

pub(super) fn file_diff_stats(diff: &FileDiff) -> (usize, usize) {
    let mut additions = 0;
    let mut deletions = 0;
    for hunk in &diff.hunks {
        for line in &hunk.lines {
            match line.origin {
                LineOrigin::Add => additions += 1,
                LineOrigin::Remove => deletions += 1,
                LineOrigin::Context => {}
            }
        }
    }
    (additions, deletions)
}

pub(super) fn git_head_sha(repo_path: &Path) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub(super) fn truncate_file_diff(mut diff: FileDiff) -> (FileDiff, bool) {
    let mut remaining = DIFF_SNAPSHOT_MAX_LINES_PER_FILE;
    let mut truncated = false;
    for hunk in &mut diff.hunks {
        if hunk.lines.len() > remaining {
            hunk.lines.truncate(remaining);
            truncated = true;
            remaining = 0;
        } else {
            remaining = remaining.saturating_sub(hunk.lines.len());
        }
    }
    if remaining == 0 {
        let original_len = diff.hunks.len();
        diff.hunks.retain(|hunk| !hunk.lines.is_empty());
        truncated |= diff.hunks.len() != original_len;
    }
    (diff, truncated)
}

pub(super) fn attachment_from_row(row: &turso::Row) -> Result<StoredAttachment> {
    Ok(StoredAttachment {
        id: parse_uuid(&row.get::<String>(0)?)?,
        agent_id: parse_uuid(&row.get::<String>(1)?)?,
        message_id: opt_text(row, 2)?.map(|id| parse_uuid(&id)).transpose()?,
        original_name: row.get(3)?,
        mime_type: opt_text(row, 4)?,
        size_bytes: i64_to_u64(row.get(5)?)?,
        sha256: row.get(6)?,
        relative_path: PathBuf::from(row.get::<String>(7)?),
        created_at: i64_to_u64(row.get(8)?)?,
        state: row.get(9)?,
    })
}

pub(super) fn project_reference_file_paths(references: &[ProjectReference]) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for reference in references {
        if let Some(path) = reference.preview_relative_path.clone() {
            paths.push(path);
        }
        let source_path = PathBuf::from(&reference.source);
        if source_path
            .components()
            .next()
            .is_some_and(|component| component.as_os_str() == "data")
        {
            paths.push(source_path);
        }
    }
    paths
}
