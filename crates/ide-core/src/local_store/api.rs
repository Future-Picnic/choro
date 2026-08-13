use super::*;

impl LocalStore {
    pub fn save_voice_turn(
        &self,
        mode: &str,
        role: &str,
        text: &str,
        agent_id: Option<Uuid>,
    ) -> Result<StoredVoiceTurn> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            insert_voice_turn_async(&conn, mode, role, text, agent_id).await
        })
    }

    pub fn load_voice_turns(&self, limit: usize) -> Result<Vec<StoredVoiceTurn>> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            load_voice_turns_async(&conn, limit).await
        })
    }

    pub fn clear_voice_turns(&self) -> Result<()> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            clear_voice_turns_async(&conn).await
        })
    }

    pub fn prune_voice_turns(&self, retention_days: u32) -> Result<()> {
        let cutoff = unix_now().saturating_sub(u64::from(retention_days) * 86_400);
        self.rt.block_on(async {
            let conn = self.connect().await?;
            prune_voice_turns_async(&conn, cutoff).await
        })
    }

    pub fn open_default() -> Result<Self> {
        // One shared runtime + one schema initialization per process. Callers
        // open the default store on every persisted message and on periodic
        // polls; constructing a fresh tokio runtime and re-running migrations
        // each time is a constant CPU cost. The init mutex keeps concurrent
        // first calls from racing the migrations; failures are not cached so
        // a transient error (e.g. disk full) can recover on a later call.
        // Note the config root is resolved once — later changes to the data
        // directory do not affect an already-opened process.
        static SHARED: std::sync::OnceLock<LocalStore> = std::sync::OnceLock::new();
        static INIT: std::sync::Mutex<()> = std::sync::Mutex::new(());
        if let Some(store) = SHARED.get() {
            return Ok(store.clone());
        }
        let _init = INIT
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(store) = SHARED.get() {
            return Ok(store.clone());
        }
        let root = AppConfig::config_path()
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let store = Self::open(root)?;
        Ok(SHARED.get_or_init(|| store).clone())
    }

    pub fn open(root: PathBuf) -> Result<Self> {
        fs::create_dir_all(&root).context("failed to create app data directory")?;
        let rt = Runtime::new().context("failed to create local store runtime")?;
        let store = Self {
            db_path: root.join("state.db"),
            root,
            rt: Arc::new(rt),
        };
        store.initialize()?;
        Ok(store)
    }

    /// Open the existing store without migrating it. Intended for helper
    /// processes (e.g. the MCP server) that run alongside the GUI: the GUI owns
    /// the schema, so a second process must not try to migrate it. Reads and
    /// targeted writes are fine (SQLite handles the concurrency). Fails if the
    /// database doesn't exist yet.
    pub fn open_existing_default() -> Result<Self> {
        let root = AppConfig::config_path()
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        Self::open_existing(root)
    }

    /// Open an existing store at an explicit app-data root without migrating
    /// it. Helper processes receive this root from the GUI so isolated app
    /// bundles never silently fall back to the main Choro database.
    pub fn open_existing(root: PathBuf) -> Result<Self> {
        let rt = Runtime::new().context("failed to create local store runtime")?;
        let store = Self {
            db_path: root.join("state.db"),
            root,
            rt: Arc::new(rt),
        };
        if !store.db_path.exists() {
            return Err(anyhow::anyhow!(
                "local store database not found at {}",
                store.db_path.display()
            ));
        }
        Ok(store)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn app_data_dir(&self) -> PathBuf {
        self.root.join("data")
    }

    pub fn agent_dir(&self, agent_id: Uuid) -> PathBuf {
        self.app_data_dir()
            .join("agents")
            .join(agent_id.to_string())
    }

    pub fn agent_attachments_dir(&self, agent_id: Uuid) -> PathBuf {
        self.agent_dir(agent_id).join("attachments")
    }

    pub fn agent_artifacts_dir(&self, agent_id: Uuid) -> PathBuf {
        self.agent_dir(agent_id).join("artifacts")
    }

    pub fn agent_raw_transcripts_dir(&self, agent_id: Uuid) -> PathBuf {
        self.agent_dir(agent_id).join("raw-transcripts")
    }

    pub fn project_icons_dir(&self, project_id: ProjectId) -> PathBuf {
        self.app_data_dir()
            .join("projects")
            .join(project_id.0.to_string())
            .join("icons")
    }

    pub fn project_references_dir(&self, project_id: ProjectId) -> PathBuf {
        self.app_data_dir()
            .join("projects")
            .join(project_id.0.to_string())
            .join("references")
    }

    pub fn project_reference_dir(&self, project_id: ProjectId, reference_id: Uuid) -> PathBuf {
        self.project_references_dir(project_id)
            .join(reference_id.to_string())
    }

    pub fn load_workspace_config(&self, fallback: AppConfig) -> Result<AppConfig> {
        let projects = self.load_projects()?;
        if projects.is_empty() {
            return Ok(fallback);
        }
        let mut config = fallback;
        config.projects = projects;
        config.project_sections = self.load_project_sections()?;
        config.active_project = config
            .active_project
            .filter(|id| config.projects.iter().any(|project| project.id == *id))
            .or_else(|| config.projects.first().map(|project| project.id));
        config
            .expanded_projects
            .retain(|id| config.projects.iter().any(|project| project.id == *id));
        Ok(config)
    }

    pub fn save_workspace_config(&self, config: &AppConfig) -> Result<()> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            execute_transaction(&conn, |conn| {
                Box::pin(async move {
                    save_project_sections_async(conn, &config.project_sections).await?;
                    save_projects_async(conn, &config.projects).await?;
                    Ok(())
                })
            })
            .await
        })
    }

    pub fn load_agents(&self) -> Result<Vec<AgentRecord>> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            load_agents_async(&conn).await
        })
    }

    pub fn save_agents(&self, agents: &[AgentRecord]) -> Result<()> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            execute_transaction(&conn, |conn| {
                Box::pin(async move { save_agents_async(conn, agents).await })
            })
            .await
        })
    }

    pub fn upsert_project_preview(
        &self,
        project_id: ProjectId,
        url: &str,
        title: &str,
        source_agent_id: Option<Uuid>,
    ) -> Result<StoredProjectPreview> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            upsert_project_preview_async(&conn, project_id, url, title, source_agent_id).await
        })
    }

    pub fn load_project_previews(
        &self,
        project_id: ProjectId,
    ) -> Result<Vec<StoredProjectPreview>> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            load_project_previews_async(&conn, project_id).await
        })
    }

    /// Remove ad-hoc Preview requests from the previous desktop session.
    /// Running scripts and simulators are discovered from live state instead.
    pub fn clear_project_previews(&self) -> Result<()> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            clear_project_previews_async(&conn).await
        })
    }

    /// Save one remembered fact. Scope `"global"` ignores `project_id`;
    /// `"project"` requires it.
    pub fn save_memory(
        &self,
        scope: &str,
        project_id: Option<ProjectId>,
        text: &str,
        source_agent_id: Option<Uuid>,
    ) -> Result<StoredMemory> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            insert_memory_async(&conn, scope, project_id, text, source_agent_id).await
        })
    }

    /// Every global memory plus the project's own, pinned first then newest —
    /// the set a session of this project starts with.
    pub fn load_memories_for_project(&self, project_id: ProjectId) -> Result<Vec<StoredMemory>> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            load_memories_for_project_async(&conn, project_id).await
        })
    }

    /// Every memory, for the Settings list.
    pub fn load_all_memories(&self) -> Result<Vec<StoredMemory>> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            load_all_memories_async(&conn).await
        })
    }

    pub fn update_memory_text(&self, id: Uuid, text: &str) -> Result<()> {
        let text = text.trim().to_string();
        anyhow::ensure!(!text.is_empty(), "memory text is empty");
        anyhow::ensure!(
            text.chars().count() <= MAX_MEMORY_TEXT_CHARS,
            "memory text must be at most {MAX_MEMORY_TEXT_CHARS} characters"
        );
        self.rt.block_on(async {
            let conn = self.connect().await?;
            conn.execute(
                "UPDATE memories SET text = ?2, updated_at = ?3 WHERE id = ?1",
                params![id.to_string(), text, u64_to_i64(unix_now())?],
            )
            .await?;
            Ok(())
        })
    }

    pub fn set_memory_enabled(&self, id: Uuid, enabled: bool) -> Result<()> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            conn.execute(
                "UPDATE memories SET enabled = ?2, updated_at = ?3 WHERE id = ?1",
                params![
                    id.to_string(),
                    bool_to_i64(enabled),
                    u64_to_i64(unix_now())?
                ],
            )
            .await?;
            Ok(())
        })
    }

    pub fn set_memory_pinned(&self, id: Uuid, pinned: bool) -> Result<()> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            conn.execute(
                "UPDATE memories SET pinned = ?2, updated_at = ?3 WHERE id = ?1",
                params![id.to_string(), bool_to_i64(pinned), u64_to_i64(unix_now())?],
            )
            .await?;
            Ok(())
        })
    }

    /// Stamp the memories included in a session's opening block. Deliberately
    /// does NOT touch `updated_at` — usage isn't an edit.
    pub fn touch_memories_last_used(&self, ids: &[Uuid]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        self.rt.block_on(async {
            let conn = self.connect().await?;
            let now = u64_to_i64(unix_now())?;
            for id in ids {
                conn.execute(
                    "UPDATE memories SET last_used_at = ?2 WHERE id = ?1",
                    params![id.to_string(), now],
                )
                .await?;
            }
            Ok(())
        })
    }

    pub fn delete_memory(&self, id: Uuid) -> Result<()> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            conn.execute("DELETE FROM memories WHERE id = ?1", [id.to_string()])
                .await?;
            Ok(())
        })
    }

    /// Undo an agent-created memory and its durable chat card as one DB
    /// transaction. The UI only removes the card after this succeeds.
    pub fn undo_memory(&self, agent_id: Uuid, memory_id: Uuid) -> Result<()> {
        let agent_id = agent_id.to_string();
        let memory_id = memory_id.to_string();
        let event_key = format!("memorized:{memory_id}");
        self.rt.block_on(async {
            let conn = self.connect().await?;
            execute_transaction(&conn, |conn| {
                Box::pin(async move {
                    let deleted = conn
                        .execute(
                            "DELETE FROM memories
                             WHERE id = ?1 AND source_agent_id = ?2",
                            params![memory_id.clone(), agent_id.clone()],
                        )
                        .await?;
                    anyhow::ensure!(deleted > 0, "memory does not belong to this agent");
                    conn.execute(
                        "DELETE FROM chat_timeline_events
                         WHERE agent_id = ?1 AND kind = 'memorized' AND event_key = ?2",
                        (agent_id, event_key),
                    )
                    .await?;
                    Ok(())
                })
            })
            .await
        })
    }

    pub fn create_project_reference(
        &self,
        project_id: ProjectId,
        kind: ProjectReferenceKind,
        title: impl Into<String>,
        source: impl Into<String>,
        notes: impl Into<String>,
        preview_source: Option<&Path>,
    ) -> Result<ProjectReference> {
        let id = Uuid::new_v4();
        let now = unix_now();
        let mut source = source.into();
        if matches!(
            kind,
            ProjectReferenceKind::Image | ProjectReferenceKind::File
        ) {
            let source_path = PathBuf::from(&source);
            if source_path.is_file() {
                source = path_to_string(&copy_project_reference_source_file(
                    self,
                    project_id,
                    id,
                    &source_path,
                )?);
            }
        }
        let preview_relative_path = match preview_source {
            Some(path) => Some(write_project_reference_preview(self, project_id, id, path)?),
            None => None,
        };
        let sort_order = self.rt.block_on(async {
            let conn = self.connect().await?;
            next_project_reference_sort_order(&conn, project_id).await
        })?;
        let reference = ProjectReference {
            id,
            project_id,
            kind,
            title: title.into(),
            source,
            preview_relative_path,
            notes: notes.into(),
            metadata_json: "{}".to_string(),
            sort_order,
            created_at: now,
            updated_at: now,
        };
        self.upsert_project_reference(&reference)?;
        Ok(reference)
    }

    pub fn create_project_image_reference_bytes(
        &self,
        project_id: ProjectId,
        title: impl Into<String>,
        notes: impl Into<String>,
        extension: &str,
        bytes: &[u8],
    ) -> Result<ProjectReference> {
        let id = Uuid::new_v4();
        let extension = extension.trim_start_matches('.').trim();
        let extension = if extension.is_empty() {
            "png"
        } else {
            extension
        };
        let source_relative_path = PathBuf::from("data")
            .join("projects")
            .join(project_id.0.to_string())
            .join("references")
            .join(id.to_string())
            .join(format!("source.{extension}"));
        let source_path = self.root.join(&source_relative_path);
        if let Some(parent) = source_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create reference directory {}", parent.display())
            })?;
        }
        fs::write(&source_path, bytes).with_context(|| {
            format!("failed to write reference image {}", source_path.display())
        })?;
        let preview_relative_path =
            write_project_reference_preview(self, project_id, id, &source_path)?;
        let now = unix_now();
        let sort_order = self.rt.block_on(async {
            let conn = self.connect().await?;
            next_project_reference_sort_order(&conn, project_id).await
        })?;
        let reference = ProjectReference {
            id,
            project_id,
            kind: ProjectReferenceKind::Image,
            title: title.into(),
            source: path_to_string(&source_relative_path),
            preview_relative_path: Some(preview_relative_path),
            notes: notes.into(),
            metadata_json: "{}".to_string(),
            sort_order,
            created_at: now,
            updated_at: now,
        };
        self.upsert_project_reference(&reference)?;
        Ok(reference)
    }

    pub fn upsert_project_reference(&self, reference: &ProjectReference) -> Result<()> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            insert_project_reference_async(&conn, reference).await
        })
    }

    pub fn load_project_references(&self, project_id: ProjectId) -> Result<Vec<ProjectReference>> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            load_project_references_async(&conn, project_id).await
        })
    }

    pub fn load_personal_tasks(&self, project_id: ProjectId) -> Result<Vec<PersonalTaskRecord>> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            load_personal_tasks_async(&conn, Some(project_id)).await
        })
    }

    pub fn create_personal_task(
        &self,
        project_id: ProjectId,
        title: impl Into<String>,
        description_markdown: impl Into<String>,
    ) -> Result<PersonalTaskRecord> {
        let title = title.into();
        let now = unix_now();
        self.rt.block_on(async {
            let conn = self.connect().await?;
            let key_number = next_personal_task_key_number(&conn, project_id).await?;
            let task = PersonalTaskRecord {
                id: Uuid::new_v4(),
                project_id,
                key_number,
                title: if title.trim().is_empty() {
                    format!("Task {key_number}")
                } else {
                    title
                },
                description_markdown: description_markdown.into(),
                status: PersonalTaskStatus::Todo,
                priority: PersonalTaskPriority::Medium,
                labels: Vec::new(),
                created_at: now,
                updated_at: now,
                archived: false,
            };
            insert_personal_task_async(&conn, &task).await?;
            Ok(task)
        })
    }

    pub fn upsert_personal_task(&self, task: &PersonalTaskRecord) -> Result<()> {
        let mut task = task.clone();
        task.updated_at = unix_now();
        self.rt.block_on(async {
            let conn = self.connect().await?;
            insert_personal_task_async(&conn, &task).await
        })
    }

    pub fn archive_personal_task(&self, task_id: Uuid, archived: bool) -> Result<()> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            conn.execute(
                "UPDATE personal_tasks SET archived = ?1, updated_at = ?2 WHERE id = ?3",
                (
                    bool_to_i64(archived),
                    u64_to_i64(unix_now())?,
                    task_id.to_string(),
                ),
            )
            .await?;
            Ok(())
        })
    }

    pub fn set_personal_task_status(
        &self,
        task_id: Uuid,
        status: PersonalTaskStatus,
    ) -> Result<()> {
        let status_label = serde_label(&status)?;
        self.rt.block_on(async {
            let conn = self.connect().await?;
            conn.execute(
                "UPDATE personal_tasks SET status = ?1, updated_at = ?2 WHERE id = ?3",
                (status_label, u64_to_i64(unix_now())?, task_id.to_string()),
            )
            .await?;
            Ok(())
        })
    }

    pub fn add_personal_task_comment(
        &self,
        task_id: Uuid,
        author: impl Into<String>,
        body: impl Into<String>,
    ) -> Result<PersonalTaskComment> {
        let comment = PersonalTaskComment {
            id: Uuid::new_v4(),
            task_id,
            author: author.into(),
            body: body.into(),
            created_at: unix_now(),
        };
        self.rt.block_on(async {
            let conn = self.connect().await?;
            conn.execute(
                "INSERT INTO personal_task_comments (id, task_id, author, body, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    comment.id.to_string(),
                    comment.task_id.to_string(),
                    comment.author.as_str(),
                    comment.body.as_str(),
                    u64_to_i64(comment.created_at)?,
                ],
            )
            .await?;
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(comment)
    }

    pub fn load_personal_task_comments(&self, task_id: Uuid) -> Result<Vec<PersonalTaskComment>> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            let mut rows = conn
                .query(
                    "SELECT id, task_id, author, body, created_at
                     FROM personal_task_comments WHERE task_id = ?1 ORDER BY created_at ASC",
                    [task_id.to_string()],
                )
                .await?;
            let mut comments = Vec::new();
            while let Some(row) = rows.next().await? {
                comments.push(PersonalTaskComment {
                    id: parse_uuid(&row.get::<String>(0)?)?,
                    task_id: parse_uuid(&row.get::<String>(1)?)?,
                    author: row.get(2)?,
                    body: row.get(3)?,
                    created_at: i64_to_u64(row.get(4)?)?,
                });
            }
            Ok(comments)
        })
    }

    pub fn delete_project_reference(&self, reference: &ProjectReference) -> Result<()> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            conn.execute(
                "DELETE FROM project_references WHERE id = ?1",
                [reference.id.to_string()],
            )
            .await?;
            Ok::<(), anyhow::Error>(())
        })?;
        let dir = self.project_reference_dir(reference.project_id, reference.id);
        if dir.exists() {
            fs::remove_dir_all(&dir)
                .with_context(|| format!("failed to remove reference files {}", dir.display()))?;
        }
        Ok(())
    }

    pub fn append_chat_message(
        &self,
        agent_id: Uuid,
        role: impl Into<String>,
        text: impl Into<String>,
        created_at: u64,
        backend_message_id: Option<String>,
    ) -> Result<StoredChatMessage> {
        let role = role.into();
        let text = text.into();
        self.rt.block_on(async {
            let conn = self.connect().await?;
            let sequence = next_sequence(&conn, "chat_messages", agent_id).await?;
            let message = StoredChatMessage {
                id: Uuid::new_v4(),
                agent_id,
                role,
                text,
                sequence,
                created_at,
                backend_message_id,
            };
            insert_chat_message_async(&conn, &message).await?;
            Ok(message)
        })
    }

    pub fn upsert_chat_message(
        &self,
        agent_id: Uuid,
        role: impl Into<String>,
        text: impl Into<String>,
        created_at: u64,
        backend_message_id: Option<String>,
    ) -> Result<StoredChatMessage> {
        let role = role.into();
        let text = text.into();
        self.rt.block_on(async {
            let conn = self.connect().await?;
            if let Some(backend_id) = backend_message_id.as_deref() {
                let mut rows = conn
                    .query(
                        "SELECT id, sequence, text, created_at FROM chat_messages
                         WHERE agent_id = ?1 AND backend_message_id = ?2
                         ORDER BY sequence DESC LIMIT 1",
                        (agent_id.to_string(), backend_id),
                    )
                    .await?;
                if let Some(row) = rows.next().await? {
                    let id = parse_uuid(&row.get::<String>(0)?)?;
                    let sequence = row.get(1)?;
                    let existing_text: String = row.get(2)?;
                    let existing_created_at = i64_to_u64(row.get(3)?)?;
                    drop(rows);
                    let keep_existing_text =
                        should_keep_existing_stream_text(&existing_text, &text);
                    let message = StoredChatMessage {
                        id,
                        agent_id,
                        role,
                        text: if keep_existing_text {
                            existing_text
                        } else {
                            text
                        },
                        sequence,
                        created_at: created_at.max(existing_created_at),
                        backend_message_id,
                    };
                    insert_chat_message_async(&conn, &message).await?;
                    return Ok(message);
                }
            }
            let sequence = next_sequence(&conn, "chat_messages", agent_id).await?;
            let message = StoredChatMessage {
                id: Uuid::new_v4(),
                agent_id,
                role,
                text,
                sequence,
                created_at,
                backend_message_id,
            };
            insert_chat_message_async(&conn, &message).await?;
            Ok(message)
        })
    }

    pub fn load_chat_messages(&self, agent_id: Uuid) -> Result<Vec<StoredChatMessage>> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            let mut rows = conn
                .query(
                    "SELECT id, agent_id, role, text, sequence, created_at, backend_message_id \
                     FROM chat_messages WHERE agent_id = ?1 ORDER BY sequence ASC",
                    [agent_id.to_string()],
                )
                .await?;
            let mut messages = Vec::new();
            while let Some(row) = rows.next().await? {
                messages.push(StoredChatMessage {
                    id: parse_uuid(&row.get::<String>(0)?)?,
                    agent_id: parse_uuid(&row.get::<String>(1)?)?,
                    role: row.get(2)?,
                    text: row.get(3)?,
                    sequence: row.get(4)?,
                    created_at: i64_to_u64(row.get(5)?)?,
                    backend_message_id: opt_text(&row, 6)?,
                });
            }
            Ok(messages)
        })
    }

    pub fn upsert_timeline_event(
        &self,
        agent_id: Uuid,
        kind: impl Into<String>,
        event_key: Option<String>,
        payload_json: impl Into<String>,
        created_at: u64,
    ) -> Result<StoredTimelineEvent> {
        let kind = kind.into();
        let payload_json = payload_json.into();
        self.rt.block_on(async {
            let conn = self.connect().await?;
            upsert_timeline_event_async(&conn, agent_id, kind, event_key, payload_json, created_at)
                .await
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn persist_timeline_event_and_chat_file_ledger(
        &self,
        agent_id: Uuid,
        kind: impl Into<String>,
        event_key: Option<String>,
        payload_json: impl Into<String>,
        created_at: u64,
        revision: u64,
        entries: &[StoredChatFileLedgerEntry],
    ) -> Result<()> {
        let kind = kind.into();
        let payload_json = payload_json.into();
        self.rt.block_on(async {
            let conn = self.connect().await?;
            execute_transaction(&conn, |conn| {
                Box::pin(async move {
                    upsert_timeline_event_async(
                        conn,
                        agent_id,
                        kind,
                        event_key,
                        payload_json,
                        created_at,
                    )
                    .await?;
                    replace_chat_file_ledger_async(conn, agent_id, revision, entries).await
                })
            })
            .await
        })
    }

    pub fn load_timeline_events(&self, agent_id: Uuid) -> Result<Vec<StoredTimelineEvent>> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            load_timeline_events_async(&conn, agent_id).await
        })
    }

    pub fn replace_chat_file_ledger(
        &self,
        agent_id: Uuid,
        revision: u64,
        entries: &[StoredChatFileLedgerEntry],
    ) -> Result<()> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            execute_transaction(&conn, |conn| {
                Box::pin(async move {
                    replace_chat_file_ledger_async(conn, agent_id, revision, entries).await
                })
            })
            .await
        })
    }

    pub fn load_chat_file_ledger(&self, agent_id: Uuid) -> Result<Option<StoredChatFileLedger>> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            load_chat_file_ledger_async(&conn, agent_id).await
        })
    }

    pub fn load_timeline_events_page(
        &self,
        agent_id: Uuid,
        before_sequence: Option<i64>,
        limit: usize,
    ) -> Result<StoredTimelinePage> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            load_timeline_events_page_async(&conn, agent_id, before_sequence, limit).await
        })
    }

    pub fn materialize_attachment_bytes(
        &self,
        agent_id: Uuid,
        original_name: impl Into<String>,
        mime_type: Option<String>,
        extension: &str,
        bytes: &[u8],
    ) -> Result<StoredAttachment> {
        let original_name = original_name.into();
        let hash = sha256_hex(bytes);
        let created_at = unix_now();
        let id = Uuid::new_v4();
        let extension = extension.trim_start_matches('.');
        let filename = if extension.is_empty() {
            id.to_string()
        } else {
            format!("{id}.{extension}")
        };
        let relative_path = PathBuf::from("data")
            .join("agents")
            .join(agent_id.to_string())
            .join("attachments")
            .join(filename);
        let absolute_path = self.root.join(&relative_path);
        if let Some(parent) = absolute_path.parent() {
            fs::create_dir_all(parent).context("failed to create agent attachment directory")?;
        }
        fs::write(&absolute_path, bytes).context("failed to write agent attachment")?;
        let attachment = StoredAttachment {
            id,
            agent_id,
            message_id: None,
            original_name,
            mime_type,
            size_bytes: bytes.len() as u64,
            sha256: hash,
            relative_path,
            created_at,
            state: "available".to_string(),
        };
        self.insert_attachment(&attachment)?;
        Ok(attachment)
    }

    pub fn materialize_attachment_file(
        &self,
        agent_id: Uuid,
        source: &Path,
        mime_type: Option<String>,
    ) -> Result<StoredAttachment> {
        let bytes = fs::read(source)
            .with_context(|| format!("failed to read attachment {}", source.display()))?;
        let original_name = source
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("attachment")
            .to_string();
        let extension = source
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("");
        self.materialize_attachment_bytes(agent_id, original_name, mime_type, extension, &bytes)
    }

    pub fn insert_attachment(&self, attachment: &StoredAttachment) -> Result<()> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            insert_attachment_async(&conn, attachment).await
        })
    }

    pub fn load_attachments(&self, agent_id: Uuid) -> Result<Vec<StoredAttachment>> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            let mut rows = conn
                .query(
                    "SELECT id, agent_id, message_id, original_name, mime_type, size_bytes, \
                     sha256, relative_path, created_at, state FROM attachments \
                     WHERE agent_id = ?1 ORDER BY created_at ASC",
                    [agent_id.to_string()],
                )
                .await?;
            let mut attachments = Vec::new();
            while let Some(row) = rows.next().await? {
                attachments.push(attachment_from_row(&row)?);
            }
            Ok(attachments)
        })
    }

    pub fn create_agent_diff_snapshot(
        &self,
        agent_id: Uuid,
        project_id: ProjectId,
        repo_path: PathBuf,
        source: impl Into<String>,
        base_sha: Option<String>,
        head_sha: Option<String>,
        commit_sha: Option<String>,
        diffs: Vec<FileDiff>,
    ) -> Result<StoredAgentDiffSnapshot> {
        let id = Uuid::new_v4();
        let now = unix_now();
        let files = diffs
            .into_iter()
            .enumerate()
            .map(|(index, diff)| {
                let (diff, truncated) = truncate_file_diff(diff);
                let (additions, deletions) = file_diff_stats(&diff);
                StoredAgentDiffFile {
                    snapshot_id: id,
                    path: diff.path.clone(),
                    additions,
                    deletions,
                    is_binary: diff.is_binary,
                    diff,
                    truncated,
                    sort_order: index as i64,
                }
            })
            .collect::<Vec<_>>();
        let snapshot = StoredAgentDiffSnapshot {
            id,
            agent_id,
            project_id,
            repo_path,
            source: source.into(),
            base_sha,
            head_sha,
            commit_sha,
            created_at: now,
            updated_at: now,
            state: "available".to_string(),
            files,
        };
        self.save_agent_diff_snapshot(&snapshot)?;
        Ok(snapshot)
    }

    pub fn save_agent_diff_snapshot(&self, snapshot: &StoredAgentDiffSnapshot) -> Result<()> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            execute_transaction(&conn, |conn| {
                Box::pin(async move { save_agent_diff_snapshot_async(conn, snapshot).await })
            })
            .await
        })
    }

    pub fn load_agent_diff_snapshot(
        &self,
        snapshot_id: Uuid,
    ) -> Result<Option<StoredAgentDiffSnapshot>> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            load_agent_diff_snapshot_async(&conn, snapshot_id).await
        })
    }

    pub fn update_agent_diff_snapshot_commit(
        &self,
        snapshot_id: Uuid,
        commit_sha: impl Into<String>,
    ) -> Result<()> {
        let commit_sha = commit_sha.into();
        self.rt.block_on(async {
            let conn = self.connect().await?;
            conn.execute(
                "UPDATE agent_diff_snapshots SET commit_sha = ?1, updated_at = ?2 WHERE id = ?3",
                (
                    commit_sha.as_str(),
                    u64_to_i64(unix_now())?,
                    snapshot_id.to_string(),
                ),
            )
            .await?;
            Ok(())
        })
    }

    pub fn backfill_agent_diff_snapshots(&self) -> Result<DiffSnapshotBackfillCounts> {
        let mut counts = DiffSnapshotBackfillCounts::default();
        for agent in self.load_agents()? {
            let events = self.load_timeline_events(agent.id)?;
            for event in events {
                if event.kind != "changed_files" {
                    continue;
                }
                counts.scanned_events += 1;
                let Ok(payload) =
                    serde_json::from_str::<BackfillChangedFilesPayload>(&event.payload_json)
                else {
                    continue;
                };
                if payload.snapshot_id.is_some() || payload.files.is_empty() {
                    continue;
                }

                let repo_path = agent.runtime_path().to_path_buf();
                let Some(match_result) = backfill_snapshot_match(&repo_path, &payload.files) else {
                    continue;
                };
                let snapshot = self.create_agent_diff_snapshot(
                    agent.id,
                    agent.project_id,
                    repo_path,
                    match_result.source,
                    match_result.base_sha,
                    match_result.head_sha,
                    match_result.commit_sha.clone(),
                    match_result.diffs,
                )?;

                let mut payload_json: serde_json::Value =
                    serde_json::from_str(&event.payload_json)?;
                payload_json["snapshot_id"] = serde_json::Value::String(snapshot.id.to_string());
                if let Some(commit_sha) = snapshot.commit_sha.clone() {
                    payload_json["commit_sha"] = serde_json::Value::String(commit_sha);
                    counts.commit_matches += 1;
                } else {
                    counts.worktree_matches += 1;
                }
                self.upsert_timeline_event(
                    agent.id,
                    event.kind,
                    event.event_key,
                    payload_json.to_string(),
                    event.created_at,
                )?;
                counts.backfilled_events += 1;
            }
        }
        Ok(counts)
    }
}
