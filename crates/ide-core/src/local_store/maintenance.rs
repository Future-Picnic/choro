use super::*;

impl LocalStore {
    pub fn export_workspace(&self, target: &Path) -> Result<()> {
        let snapshot = self.export_snapshot()?;
        let file = File::create(target)
            .with_context(|| format!("failed to create export archive {}", target.display()))?;
        let mut zip = ZipWriter::new(file);
        let options = FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let mut checksums = Vec::new();

        write_zip_json(
            &mut zip,
            options,
            "workspace.json",
            &snapshot.workspace,
            &mut checksums,
        )?;
        write_zip_jsonl(
            &mut zip,
            options,
            "agents.jsonl",
            &snapshot.agents,
            &mut checksums,
        )?;
        write_zip_jsonl(
            &mut zip,
            options,
            "messages.jsonl",
            &snapshot.messages,
            &mut checksums,
        )?;
        write_zip_jsonl(
            &mut zip,
            options,
            "timeline.jsonl",
            &snapshot.timeline_events,
            &mut checksums,
        )?;
        write_zip_jsonl(
            &mut zip,
            options,
            "chat_file_ledgers.jsonl",
            &snapshot.chat_file_ledgers,
            &mut checksums,
        )?;
        write_zip_jsonl(
            &mut zip,
            options,
            "memories.jsonl",
            &snapshot.memories,
            &mut checksums,
        )?;
        write_zip_jsonl(
            &mut zip,
            options,
            "agent_summaries.jsonl",
            &snapshot.agent_summaries,
            &mut checksums,
        )?;
        write_zip_jsonl(
            &mut zip,
            options,
            "agent_messages.jsonl",
            &snapshot.agent_messages,
            &mut checksums,
        )?;
        write_zip_jsonl(
            &mut zip,
            options,
            "attachments.jsonl",
            &snapshot.attachments,
            &mut checksums,
        )?;
        write_zip_jsonl(
            &mut zip,
            options,
            "project_references.jsonl",
            &snapshot.project_references,
            &mut checksums,
        )?;
        write_zip_jsonl(
            &mut zip,
            options,
            "personal_tasks.jsonl",
            &snapshot.personal_tasks,
            &mut checksums,
        )?;
        let diff_snapshot_rows = snapshot
            .diff_snapshots
            .iter()
            .map(StoredAgentDiffSnapshotRow::from)
            .collect::<Vec<_>>();
        let diff_files = snapshot
            .diff_snapshots
            .iter()
            .flat_map(|snapshot| snapshot.files.clone())
            .collect::<Vec<_>>();
        write_zip_jsonl(
            &mut zip,
            options,
            "diff_snapshots.jsonl",
            &diff_snapshot_rows,
            &mut checksums,
        )?;
        write_zip_jsonl(
            &mut zip,
            options,
            "diff_files.jsonl",
            &diff_files,
            &mut checksums,
        )?;
        write_zip_jsonl(
            &mut zip,
            options,
            "penpot_connections.jsonl",
            &snapshot.penpot_connections,
            &mut checksums,
        )?;
        write_zip_jsonl(
            &mut zip,
            options,
            "penpot_bindings.jsonl",
            &snapshot.penpot_bindings,
            &mut checksums,
        )?;
        write_zip_jsonl(
            &mut zip,
            options,
            "penpot_designs.jsonl",
            &snapshot.penpot_designs,
            &mut checksums,
        )?;
        write_zip_jsonl(
            &mut zip,
            options,
            "penpot_conversations.jsonl",
            &snapshot.penpot_conversations,
            &mut checksums,
        )?;
        write_zip_jsonl(
            &mut zip,
            options,
            "orbit_modules.jsonl",
            &snapshot.orbit_modules,
            &mut checksums,
        )?;
        write_zip_jsonl(
            &mut zip,
            options,
            "orbit_bindings.jsonl",
            &snapshot.orbit_bindings,
            &mut checksums,
        )?;
        write_zip_jsonl(
            &mut zip,
            options,
            "orbit_records.jsonl",
            &snapshot.orbit_records,
            &mut checksums,
        )?;
        write_zip_jsonl(
            &mut zip,
            options,
            "orbit_invocations.jsonl",
            &snapshot.orbit_invocations,
            &mut checksums,
        )?;
        write_zip_jsonl(
            &mut zip,
            options,
            "orbit_mutation_batches.jsonl",
            &snapshot.orbit_mutation_batches,
            &mut checksums,
        )?;

        for attachment in &snapshot.attachments {
            let absolute = self.root.join(&attachment.relative_path);
            if !absolute.is_file() {
                continue;
            }
            let archive_path = PathBuf::from("files").join(&attachment.relative_path);
            let archive_path = archive_path.to_string_lossy().replace('\\', "/");
            let mut bytes = Vec::new();
            File::open(&absolute)
                .with_context(|| format!("failed to open attachment {}", absolute.display()))?
                .read_to_end(&mut bytes)
                .context("failed to read attachment")?;
            checksums.push(ExportChecksum {
                path: archive_path.clone(),
                sha256: sha256_hex(&bytes),
            });
            zip.start_file(archive_path, options)?;
            zip.write_all(&bytes)?;
        }
        let mut exported_reference_files = HashSet::new();
        for relative_path in project_reference_file_paths(&snapshot.project_references) {
            if !exported_reference_files.insert(relative_path.clone()) {
                continue;
            }
            let absolute = self.root.join(&relative_path);
            if !absolute.is_file() {
                continue;
            }
            let archive_path = PathBuf::from("files").join(&relative_path);
            let archive_path = archive_path.to_string_lossy().replace('\\', "/");
            let mut bytes = Vec::new();
            File::open(&absolute)
                .with_context(|| format!("failed to open reference file {}", absolute.display()))?
                .read_to_end(&mut bytes)
                .context("failed to read reference file")?;
            checksums.push(ExportChecksum {
                path: archive_path.clone(),
                sha256: sha256_hex(&bytes),
            });
            zip.start_file(archive_path, options)?;
            zip.write_all(&bytes)?;
        }

        let manifest = ExportManifest {
            format_version: EXPORT_FORMAT_VERSION,
            app: crate::branding::APP_ID.to_string(),
            exported_at: unix_now(),
            counts: ExportCounts {
                projects: snapshot.workspace.projects.len(),
                project_references: snapshot.project_references.len(),
                personal_tasks: snapshot.personal_tasks.len(),
                agents: snapshot.agents.len(),
                messages: snapshot.messages.len(),
                timeline_events: snapshot.timeline_events.len(),
                chat_file_ledgers: snapshot.chat_file_ledgers.len(),
                memories: snapshot.memories.len(),
                agent_summaries: snapshot.agent_summaries.len(),
                agent_messages: snapshot.agent_messages.len(),
                attachments: snapshot.attachments.len(),
                diff_snapshots: diff_snapshot_rows.len(),
                diff_files: diff_files.len(),
                penpot_connections: snapshot.penpot_connections.len(),
                penpot_bindings: snapshot.penpot_bindings.len(),
                penpot_designs: snapshot.penpot_designs.len(),
                penpot_conversations: snapshot.penpot_conversations.len(),
                orbit_modules: snapshot.orbit_modules.len(),
                orbit_bindings: snapshot.orbit_bindings.len(),
                orbit_records: snapshot.orbit_records.len(),
                orbit_invocations: snapshot.orbit_invocations.len(),
                orbit_mutation_batches: snapshot.orbit_mutation_batches.len(),
            },
            checksums,
        };
        zip.start_file("manifest.json", options)?;
        zip.write_all(&serde_json::to_vec_pretty(&manifest)?)?;
        zip.finish()?;
        Ok(())
    }

    pub fn import_workspace_replace(&self, archive: &Path) -> Result<PathBuf> {
        let backup = self.backup_current_data()?;
        let file = File::open(archive)
            .with_context(|| format!("failed to open import archive {}", archive.display()))?;
        let mut zip = ZipArchive::new(file).context("failed to read import archive")?;
        let manifest: ExportManifest = read_zip_json(&mut zip, "manifest.json")?;
        anyhow::ensure!(
            (1..=EXPORT_FORMAT_VERSION).contains(&manifest.format_version),
            "unsupported export format version {}",
            manifest.format_version
        );
        let workspace: AppConfig = read_zip_json(&mut zip, "workspace.json")?;
        let agents: Vec<AgentRecord> = read_zip_jsonl(&mut zip, "agents.jsonl")?;
        let messages: Vec<StoredChatMessage> = read_zip_jsonl(&mut zip, "messages.jsonl")?;
        let timeline_events: Vec<StoredTimelineEvent> = read_zip_jsonl(&mut zip, "timeline.jsonl")?;
        let chat_file_ledgers: Vec<StoredChatFileLedger> =
            read_zip_jsonl_optional(&mut zip, "chat_file_ledgers.jsonl")?;
        let memories: Vec<StoredMemory> = read_zip_jsonl_optional(&mut zip, "memories.jsonl")?;
        let agent_summaries: Vec<StoredAgentSummary> =
            read_zip_jsonl_optional(&mut zip, "agent_summaries.jsonl")?;
        let agent_messages: Vec<StoredAgentMessage> =
            read_zip_jsonl_optional(&mut zip, "agent_messages.jsonl")?;
        let attachments: Vec<StoredAttachment> = read_zip_jsonl(&mut zip, "attachments.jsonl")?;
        let project_references: Vec<ProjectReference> =
            read_zip_jsonl_optional(&mut zip, "project_references.jsonl")?;
        let personal_tasks: Vec<PersonalTaskRecord> =
            read_zip_jsonl_optional(&mut zip, "personal_tasks.jsonl")?;
        let diff_snapshot_rows: Vec<StoredAgentDiffSnapshotRow> =
            read_zip_jsonl_optional(&mut zip, "diff_snapshots.jsonl")?;
        let diff_files: Vec<StoredAgentDiffFile> =
            read_zip_jsonl_optional(&mut zip, "diff_files.jsonl")?;
        let diff_snapshots = inflate_diff_snapshots(diff_snapshot_rows, diff_files);
        let penpot_connections: Vec<StoredPenpotConnection> =
            read_zip_jsonl_optional(&mut zip, "penpot_connections.jsonl")?;
        let penpot_bindings: Vec<StoredProjectPenpotBinding> =
            read_zip_jsonl_optional(&mut zip, "penpot_bindings.jsonl")?;
        let penpot_designs: Vec<StoredPenpotDesign> =
            read_zip_jsonl_optional(&mut zip, "penpot_designs.jsonl")?;
        let penpot_conversations: Vec<StoredPenpotDesignConversation> =
            read_zip_jsonl_optional(&mut zip, "penpot_conversations.jsonl")?;
        let orbit_modules: Vec<OrbitModuleDefinition> =
            read_zip_jsonl_optional(&mut zip, "orbit_modules.jsonl")?;
        let orbit_bindings: Vec<OrbitProjectBinding> =
            read_zip_jsonl_optional(&mut zip, "orbit_bindings.jsonl")?;
        let orbit_records: Vec<OrbitRecord> =
            read_zip_jsonl_optional(&mut zip, "orbit_records.jsonl")?;
        let orbit_invocations: Vec<OrbitInvocation> =
            read_zip_jsonl_optional(&mut zip, "orbit_invocations.jsonl")?;
        let orbit_mutation_batches: Vec<OrbitMutationBatch> =
            read_zip_jsonl_optional(&mut zip, "orbit_mutation_batches.jsonl")?;

        let data_root = self.app_data_dir();
        if data_root.exists() {
            fs::remove_dir_all(&data_root).context("failed to clear data root before import")?;
        }
        fs::create_dir_all(&data_root).context("failed to recreate data root")?;

        for index in 0..zip.len() {
            let mut entry = zip.by_index(index)?;
            let name = entry.name().to_string();
            let Some(relative) = name.strip_prefix("files/") else {
                continue;
            };
            let target = self.root.join(relative);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut output = File::create(&target)?;
            std::io::copy(&mut entry, &mut output)?;
        }

        let workspace_for_db = workspace.clone();
        self.rt.block_on(async {
            let conn = self.connect().await?;
            execute_transaction(&conn, |conn| {
                Box::pin(async move {
                    clear_imported_tables(conn).await?;
                    save_project_sections_async(conn, &workspace_for_db.project_sections).await?;
                    save_projects_async(conn, &workspace_for_db.projects).await?;
                    for module in &orbit_modules {
                        orbit::save_orbit_module_async(conn, module).await?;
                        conn.execute(
                            "UPDATE orbit_modules
                             SET revision = ?2, archived = ?3, created_at = ?4, updated_at = ?5
                             WHERE id = ?1",
                            params![
                                module.id.to_string(),
                                u64_to_i64(module.revision.max(1))?,
                                bool_to_i64(module.archived),
                                u64_to_i64(module.created_at)?,
                                u64_to_i64(module.updated_at)?,
                            ],
                        )
                        .await?;
                    }
                    for binding in &orbit_bindings {
                        conn.execute(
                            "INSERT INTO orbit_project_modules
                             (project_id, module_key, enabled, sort_order, data_revision,
                              created_at, updated_at)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                            params![
                                binding.project_id.0.to_string(),
                                binding.module.storage_key(),
                                bool_to_i64(binding.enabled),
                                binding.sort_order,
                                u64_to_i64(binding.data_revision)?,
                                u64_to_i64(binding.created_at)?,
                                u64_to_i64(binding.updated_at)?,
                            ],
                        )
                        .await?;
                    }
                    for connection in &penpot_connections {
                        insert_penpot_connection_async(conn, connection, false).await?;
                    }
                    for binding in &penpot_bindings {
                        insert_project_penpot_binding_async(conn, binding).await?;
                    }
                    for design in &penpot_designs {
                        insert_penpot_design_async(conn, design).await?;
                    }
                    for conversation in &penpot_conversations {
                        insert_penpot_conversation_async(conn, conversation).await?;
                    }
                    for reference in &project_references {
                        insert_project_reference_async(conn, reference).await?;
                    }
                    for task in &personal_tasks {
                        insert_personal_task_async(conn, task).await?;
                    }
                    save_agents_async(conn, &agents).await?;
                    for record in &orbit_records {
                        conn.execute(
                            "INSERT INTO orbit_records
                             (id, project_id, module_id, section_value, values_json, record_key,
                              source_agent_id, source_batch_id, created_at, updated_at)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                            params![
                                record.id.to_string(),
                                record.project_id.0.to_string(),
                                record.module_id.to_string(),
                                record.section.as_deref(),
                                serde_json::to_string(&record.values)?,
                                record.record_key.as_str(),
                                record.source_agent_id.map(|id| id.to_string()),
                                record.source_batch_id.map(|id| id.to_string()),
                                u64_to_i64(record.created_at)?,
                                u64_to_i64(record.updated_at)?,
                            ],
                        )
                        .await?;
                    }
                    for invocation in &orbit_invocations {
                        conn.execute(
                            "INSERT INTO orbit_invocations
                             (id, agent_id, project_id, module_id, module_revision, data_revision,
                              expires_at, completed_at, created_at)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                            params![
                                invocation.id.to_string(),
                                invocation.agent_id.to_string(),
                                invocation.project_id.0.to_string(),
                                invocation.module_id.to_string(),
                                u64_to_i64(invocation.module_revision)?,
                                u64_to_i64(invocation.data_revision)?,
                                u64_to_i64(invocation.expires_at)?,
                                invocation.completed_at.map(u64_to_i64).transpose()?,
                                u64_to_i64(invocation.created_at)?,
                            ],
                        )
                        .await?;
                    }
                    for batch in &orbit_mutation_batches {
                        conn.execute(
                            "INSERT INTO orbit_mutation_batches
                             (id, invocation_id, agent_id, project_id, module_id,
                              revision_before, revision_after, before_json, after_json,
                              inserted_count, updated_count, deleted_count, created_at, undone_at)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                            params![
                                batch.id.to_string(),
                                batch.invocation_id.to_string(),
                                batch.agent_id.to_string(),
                                batch.project_id.0.to_string(),
                                batch.module_id.to_string(),
                                u64_to_i64(batch.revision_before)?,
                                u64_to_i64(batch.revision_after)?,
                                serde_json::to_string(&batch.before)?,
                                serde_json::to_string(&batch.after)?,
                                i64::try_from(batch.inserted)?,
                                i64::try_from(batch.updated)?,
                                i64::try_from(batch.deleted)?,
                                u64_to_i64(batch.created_at)?,
                                batch.undone_at.map(u64_to_i64).transpose()?,
                            ],
                        )
                        .await?;
                    }
                    for message in &messages {
                        insert_chat_message_async(conn, message).await?;
                    }
                    for event in &timeline_events {
                        insert_timeline_event_async(conn, event).await?;
                    }
                    for ledger in &chat_file_ledgers {
                        replace_chat_file_ledger_async(
                            conn,
                            ledger.agent_id,
                            ledger.revision,
                            &ledger.entries,
                        )
                        .await?;
                        if ledger.entries.is_empty() {
                            conn.execute(
                                "UPDATE chat_file_ledgers SET updated_at = ?1 WHERE agent_id = ?2",
                                (u64_to_i64(ledger.updated_at)?, ledger.agent_id.to_string()),
                            )
                            .await?;
                        }
                    }
                    anyhow::ensure!(
                        memories.len() <= MAX_MEMORY_ROWS,
                        "import contains more than {MAX_MEMORY_ROWS} memories"
                    );
                    for memory in &memories {
                        insert_stored_memory_async(conn, memory).await?;
                    }
                    for summary in &agent_summaries {
                        insert_stored_agent_summary_async(conn, summary).await?;
                    }
                    for message in &agent_messages {
                        insert_stored_agent_message_async(conn, message).await?;
                    }
                    for attachment in &attachments {
                        insert_attachment_async(conn, attachment).await?;
                    }
                    for snapshot in &diff_snapshots {
                        save_agent_diff_snapshot_async(conn, snapshot).await?;
                    }
                    Ok(())
                })
            })
            .await
        })?;
        workspace.save_to(&self.root.join("config.json"))?;
        Ok(backup)
    }

    pub(super) fn initialize(&self) -> Result<()> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            run_migrations(&conn).await?;
            let imported = get_meta(&conn, "legacy_imported").await?.is_some();
            if !imported {
                let legacy_config = AppConfig::load_from(&self.root.join("config.json"));
                let legacy_agents =
                    AgentStoreFile::load_from(&self.root.join("agents.json")).agents;
                execute_transaction(&conn, |conn| {
                    Box::pin(async move {
                        save_project_sections_async(conn, &legacy_config.project_sections).await?;
                        save_projects_async(conn, &legacy_config.projects).await?;
                        save_agents_async(conn, &legacy_agents).await?;
                        set_meta(conn, "legacy_imported", "true").await?;
                        Ok(())
                    })
                })
                .await?;
            } else {
                let legacy_config = AppConfig::load_from(&self.root.join("config.json"));
                let legacy_agents =
                    AgentStoreFile::load_from(&self.root.join("agents.json")).agents;
                let projects_empty = table_count(&conn, "projects").await? == 0;
                let agents_empty = table_count(&conn, "agents").await? == 0;
                if (projects_empty && !legacy_config.projects.is_empty())
                    || (agents_empty && !legacy_agents.is_empty())
                {
                    execute_transaction(&conn, |conn| {
                        Box::pin(async move {
                            if projects_empty && !legacy_config.projects.is_empty() {
                                save_project_sections_async(conn, &legacy_config.project_sections)
                                    .await?;
                                save_projects_async(conn, &legacy_config.projects).await?;
                            }
                            if agents_empty && !legacy_agents.is_empty() {
                                save_agents_async(conn, &legacy_agents).await?;
                            }
                            Ok(())
                        })
                    })
                    .await?;
                }
            }
            Ok(())
        })
    }

    pub(super) async fn connect(&self) -> Result<Connection> {
        let db_path = self.db_path.to_string_lossy().to_string();
        let mut attempt = 0_u8;
        let db = loop {
            match Builder::new_local(&db_path)
                .experimental_index_method(true)
                .build()
                .await
            {
                Ok(db) => break db,
                Err(error)
                    if attempt < 40 && error.to_string().to_ascii_lowercase().contains("lock") =>
                {
                    attempt += 1;
                    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                }
                Err(error) => {
                    return Err(error).context("failed to open local Turso database");
                }
            }
        };
        let conn = db
            .connect()
            .context("failed to connect to local Turso database")?;
        // Wait for a held lock instead of failing immediately with
        // "database is locked" when another connection is mid-write.
        conn.execute("PRAGMA busy_timeout = 5000", ()).await?;
        conn.execute("PRAGMA foreign_keys = ON", ()).await?;
        Ok(conn)
    }

    pub(super) fn load_projects(&self) -> Result<Vec<Project>> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            load_projects_async(&conn).await
        })
    }

    pub(super) fn load_project_sections(&self) -> Result<Vec<ProjectSection>> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            load_project_sections_async(&conn).await
        })
    }

    pub(super) fn export_snapshot(&self) -> Result<ExportSnapshot> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            let mut workspace = AppConfig::load_from(&self.root.join("config.json"));
            let projects = load_projects_async(&conn).await?;
            if !projects.is_empty() {
                workspace.projects = projects;
                workspace.project_sections = load_project_sections_async(&conn).await?;
                workspace.active_project = workspace
                    .active_project
                    .filter(|id| workspace.projects.iter().any(|project| project.id == *id))
                    .or_else(|| workspace.projects.first().map(|project| project.id));
                workspace
                    .expanded_projects
                    .retain(|id| workspace.projects.iter().any(|project| project.id == *id));
            }
            let orbit_modules = orbit::load_orbit_modules_async(&conn, true).await?;
            let project_ids = workspace
                .projects
                .iter()
                .map(|project| project.id)
                .collect::<Vec<_>>();
            let mut bindings_by_project =
                orbit::load_orbit_bindings_for_projects_async(&conn, &project_ids).await?;
            let orbit_bindings = project_ids
                .into_iter()
                .flat_map(|project_id| bindings_by_project.remove(&project_id).unwrap_or_default())
                .collect();
            let orbit_records = orbit::load_all_orbit_records_async(&conn).await?;
            Ok(ExportSnapshot {
                workspace,
                agents: load_agents_async(&conn).await?,
                messages: load_all_messages_async(&conn).await?,
                timeline_events: load_all_timeline_events_async(&conn).await?,
                chat_file_ledgers: load_all_chat_file_ledgers_async(&conn).await?,
                memories: load_all_memories_async(&conn).await?,
                agent_summaries: load_all_agent_summaries_async(&conn).await?,
                agent_messages: load_all_agent_messages_async(&conn).await?,
                attachments: load_all_attachments_async(&conn).await?,
                project_references: load_all_project_references_async(&conn).await?,
                personal_tasks: load_personal_tasks_async(&conn, None).await?,
                diff_snapshots: load_all_agent_diff_snapshots_async(&conn).await?,
                penpot_connections: load_all_penpot_connections_async(&conn).await?,
                penpot_bindings: load_all_project_penpot_bindings_async(&conn).await?,
                penpot_designs: load_penpot_designs_async(&conn, None).await?,
                penpot_conversations: load_all_penpot_conversations_async(&conn).await?,
                orbit_modules,
                orbit_bindings,
                orbit_records,
                orbit_invocations: orbit::load_all_orbit_invocations_async(&conn).await?,
                orbit_mutation_batches: orbit::load_all_orbit_mutation_batches_async(&conn).await?,
            })
        })
    }

    pub(super) fn backup_current_data(&self) -> Result<PathBuf> {
        let backup = self
            .root
            .join(format!("backup-before-import-{}", unix_now()));
        fs::create_dir_all(&backup).context("failed to create import backup directory")?;
        for name in [
            "state.db",
            "config.json",
            "agents.json",
            "penpot.json",
            "data",
        ] {
            let source = self.root.join(name);
            if !source.exists() {
                continue;
            }
            let target = backup.join(name);
            if source.is_dir() {
                copy_dir_recursive(&source, &target)?;
            } else {
                fs::copy(&source, &target).with_context(|| {
                    format!("failed to copy {} into import backup", source.display())
                })?;
            }
        }
        Ok(backup)
    }
}

pub(super) struct ExportSnapshot {
    workspace: AppConfig,
    project_references: Vec<ProjectReference>,
    personal_tasks: Vec<PersonalTaskRecord>,
    agents: Vec<AgentRecord>,
    messages: Vec<StoredChatMessage>,
    timeline_events: Vec<StoredTimelineEvent>,
    chat_file_ledgers: Vec<StoredChatFileLedger>,
    memories: Vec<StoredMemory>,
    agent_summaries: Vec<StoredAgentSummary>,
    agent_messages: Vec<StoredAgentMessage>,
    attachments: Vec<StoredAttachment>,
    diff_snapshots: Vec<StoredAgentDiffSnapshot>,
    penpot_connections: Vec<StoredPenpotConnection>,
    penpot_bindings: Vec<StoredProjectPenpotBinding>,
    penpot_designs: Vec<StoredPenpotDesign>,
    penpot_conversations: Vec<StoredPenpotDesignConversation>,
    orbit_modules: Vec<OrbitModuleDefinition>,
    orbit_bindings: Vec<OrbitProjectBinding>,
    orbit_records: Vec<OrbitRecord>,
    orbit_invocations: Vec<OrbitInvocation>,
    orbit_mutation_batches: Vec<OrbitMutationBatch>,
}
