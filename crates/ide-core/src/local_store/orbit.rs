//! Choro Orbit definitions, project bindings, records, and scoped agent writes.

use super::*;

const MAX_MODULE_NAME_CHARS: usize = 80;
const MAX_MODULE_DESCRIPTION_CHARS: usize = 500;
const MAX_AGENT_JOB_CHARS: usize = 24_000;
const MAX_FIELDS: usize = 24;
const MAX_RECORD_CHANGES: usize = 500;
const MAX_SHORT_TEXT_CHARS: usize = 240;
const MAX_LONG_TEXT_CHARS: usize = 12_000;
const MAX_LIST_ITEMS: usize = 100;
const MAX_LIST_ITEM_CHARS: usize = 500;
const ORBIT_INVOCATION_TTL_SECONDS: u64 = 2 * 60 * 60;

const ANALYTICS_AGENT_JOB: &str = r#"You are the Analytics Instrumentation Agent for this project's Orbit Analytics module.

Your job is to implement product analytics events end-to-end using whatever analytics system already exists in the codebase (Amplitude, Mixpanel, Firebase Analytics, Segment, PostHog, or another provider) and keep the Orbit Analytics records exact.

Do not ask for information you can discover yourself. Orbit is the analytics lexicon; do not search for, create, or update a separate lexicon file in the repository.

For each request:

1. DISCOVER
- Call `orbit_read` with the authorized invocation ID before changing Orbit records.
- Identify the analytics provider, wrapper, hooks, helpers, constants, types, and existing conventions in the project.
- Search for similar events and determine the correct firing location and condition.

2. IMPLEMENT
- Add or modify the requested event at the correct point in the product flow.
- Add only the requested properties.
- Follow existing naming conventions and analytics abstractions.
- Ensure events fire exactly when intended and are not duplicated.
- Do not change unrelated behavior.

3. UPDATE ORBIT
- Every event you add or modify must be reflected through `orbit_apply_changes`.
- Area: product area, feature, screen, or logical section.
- Name: exact event name sent to the provider.
- What It Does: concise description of what it represents and when it fires.
- Properties: every property sent, with its meaning and type/value where useful.
- Notes: meaningful edge cases, firing conditions, limitations, or implementation details; otherwise leave it empty.
- Code and Orbit names and properties must match exactly.

4. VERIFY
- Confirm every requested event and property is implemented.
- Confirm firing behavior is correct and not duplicated.
- Confirm Orbit matches the implementation.
- Run relevant typechecks, linting, and tests and fix issues introduced by the work.

Rules:
- Research first, then act autonomously.
- Do not invent a new analytics architecture or rename existing events unless asked.
- Never access Choro's database directly. Use only `orbit_read` and `orbit_apply_changes`.
- Never send secrets, credentials, or unnecessary PII.
- Make the actual code changes; do not merely explain them.
- Delete Orbit records only when the user explicitly asks to remove events or requests a full reconciliation that proves the events no longer exist.

The task is complete only when code, firing behavior, and Orbit agree. Finish with a short summary and genuine unresolved issues."#;

pub fn normalize_orbit_field_key(label: &str) -> String {
    let mut key = String::new();
    let mut separator = false;
    for ch in label.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            key.push(ch.to_ascii_lowercase());
            separator = false;
        } else if !key.is_empty() && !separator {
            key.push('_');
            separator = true;
        }
    }
    while key.ends_with('_') {
        key.pop();
    }
    key
}

pub fn blank_orbit_module() -> OrbitModuleDefinition {
    let now = unix_now();
    OrbitModuleDefinition {
        id: Uuid::new_v4(),
        name: String::new(),
        description: String::new(),
        view_type: OrbitViewType::GroupedTable,
        section_key: None,
        section_label: None,
        agent_job: String::new(),
        revision: 0,
        archived: false,
        fields: vec![OrbitFieldDefinition {
            id: Uuid::new_v4(),
            key: "name".to_string(),
            label: "Name".to_string(),
            kind: OrbitFieldKind::ShortText,
            primary: true,
            sort_order: 0,
            archived: false,
        }],
        created_at: now,
        updated_at: now,
    }
}

pub fn analytics_orbit_template() -> OrbitModuleDefinition {
    let now = unix_now();
    let mut module = OrbitModuleDefinition {
        id: Uuid::new_v4(),
        name: "Analytics".to_string(),
        description:
            "Shows product analytics events, when they fire, and the properties they send."
                .to_string(),
        view_type: OrbitViewType::GroupedTable,
        section_key: Some("area".to_string()),
        section_label: Some("Area".to_string()),
        agent_job: ANALYTICS_AGENT_JOB.to_string(),
        revision: 0,
        archived: false,
        fields: Vec::new(),
        created_at: now,
        updated_at: now,
    };
    module.fields = [
        ("name", "Name", OrbitFieldKind::ShortText, true),
        (
            "what_it_does",
            "What It Does",
            OrbitFieldKind::LongText,
            false,
        ),
        ("properties", "Properties", OrbitFieldKind::List, false),
        ("notes", "Notes", OrbitFieldKind::LongText, false),
    ]
    .into_iter()
    .enumerate()
    .map(
        |(index, (key, label, kind, primary))| OrbitFieldDefinition {
            id: Uuid::new_v4(),
            key: key.to_string(),
            label: label.to_string(),
            kind,
            primary,
            sort_order: index as i64,
            archived: false,
        },
    )
    .collect();
    module
}

pub fn validate_orbit_module(module: &OrbitModuleDefinition) -> Result<()> {
    let name = module.name.trim();
    anyhow::ensure!(!name.is_empty(), "module name is required");
    anyhow::ensure!(
        name.chars().count() <= MAX_MODULE_NAME_CHARS,
        "module name must be at most {MAX_MODULE_NAME_CHARS} characters"
    );
    anyhow::ensure!(
        !module.description.trim().is_empty(),
        "module description is required"
    );
    anyhow::ensure!(
        module.description.chars().count() <= MAX_MODULE_DESCRIPTION_CHARS,
        "module description must be at most {MAX_MODULE_DESCRIPTION_CHARS} characters"
    );
    anyhow::ensure!(!module.agent_job.trim().is_empty(), "agent job is required");
    anyhow::ensure!(
        module.agent_job.chars().count() <= MAX_AGENT_JOB_CHARS,
        "agent job must be at most {MAX_AGENT_JOB_CHARS} characters"
    );

    match (&module.section_key, &module.section_label) {
        (None, None) => {}
        (Some(key), Some(label)) => {
            anyhow::ensure!(!label.trim().is_empty(), "section label is empty");
            anyhow::ensure!(
                normalize_orbit_field_key(key) == key.as_str() && !key.is_empty(),
                "section key is invalid"
            );
        }
        _ => return Err(anyhow!("section key and label must be configured together")),
    }

    let active = module
        .fields
        .iter()
        .filter(|field| !field.archived)
        .collect::<Vec<_>>();
    anyhow::ensure!(!active.is_empty(), "at least one field is required");
    anyhow::ensure!(
        active.len() <= MAX_FIELDS,
        "modules support at most {MAX_FIELDS} fields"
    );
    anyhow::ensure!(
        active.iter().filter(|field| field.primary).count() == 1,
        "exactly one primary field is required"
    );
    let primary = active.iter().find(|field| field.primary).unwrap();
    anyhow::ensure!(
        primary.kind == OrbitFieldKind::ShortText,
        "the primary field must use short text"
    );
    let mut keys = HashSet::new();
    for field in active {
        anyhow::ensure!(!field.label.trim().is_empty(), "field label is empty");
        anyhow::ensure!(
            normalize_orbit_field_key(&field.key) == field.key && !field.key.is_empty(),
            "field key '{}' is invalid",
            field.key
        );
        anyhow::ensure!(keys.insert(field.key.as_str()), "field keys must be unique");
        anyhow::ensure!(
            module.section_key.as_deref() != Some(field.key.as_str()),
            "section and regular field keys must be different"
        );
    }
    Ok(())
}

impl LocalStore {
    pub fn load_orbit_snapshot(&self, projects: &[ProjectId]) -> Result<OrbitSnapshot> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            load_orbit_snapshot_async(&conn, projects).await
        })
    }

    pub fn load_orbit_project_module_snapshot(
        &self,
        project_id: ProjectId,
        module_id: Uuid,
    ) -> Result<OrbitProjectModuleSnapshot> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            let mut bindings = load_orbit_bindings_for_projects_async(&conn, &[project_id]).await?;
            let binding = bindings
                .remove(&project_id)
                .and_then(|bindings| {
                    bindings
                        .into_iter()
                        .find(|binding| binding.module == OrbitModuleId::Custom(module_id))
                })
                .context("Orbit module is not added to this project")?;
            let records = if binding.enabled {
                load_orbit_records_async(&conn, project_id, module_id).await?
            } else {
                Vec::new()
            };
            Ok(OrbitProjectModuleSnapshot { binding, records })
        })
    }

    pub fn load_orbit_module_project_snapshots(
        &self,
        projects: &[ProjectId],
        module_id: Uuid,
    ) -> Result<Vec<OrbitProjectModuleSnapshot>> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            let bindings = load_orbit_bindings_for_projects_async(&conn, projects).await?;
            let module_bindings = bindings
                .into_values()
                .flatten()
                .filter(|binding| binding.module == OrbitModuleId::Custom(module_id))
                .collect::<Vec<_>>();
            let active_scopes = module_bindings
                .iter()
                .filter(|binding| binding.enabled)
                .map(|binding| (binding.project_id, module_id))
                .collect::<HashSet<_>>();
            let mut records = load_orbit_records_for_scopes_async(&conn, &active_scopes).await?;
            Ok(module_bindings
                .into_iter()
                .map(|binding| OrbitProjectModuleSnapshot {
                    records: records
                        .remove(&(binding.project_id, module_id))
                        .unwrap_or_default(),
                    binding,
                })
                .collect())
        })
    }

    pub fn load_orbit_modules(&self, include_archived: bool) -> Result<Vec<OrbitModuleDefinition>> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            load_orbit_modules_async(&conn, include_archived).await
        })
    }

    pub fn load_orbit_module(&self, id: Uuid) -> Result<Option<OrbitModuleDefinition>> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            load_orbit_module_async(&conn, id).await
        })
    }

    pub fn save_orbit_module(
        &self,
        module: &OrbitModuleDefinition,
    ) -> Result<OrbitModuleDefinition> {
        validate_orbit_module(module)?;
        self.rt.block_on(async {
            let conn = self.connect().await?;
            let saved = std::cell::RefCell::new(None);
            execute_transaction(&conn, |conn| {
                let saved = &saved;
                Box::pin(async move {
                    saved.replace(Some(save_orbit_module_async(conn, module).await?));
                    Ok(())
                })
            })
            .await?;
            saved
                .into_inner()
                .context("Orbit module transaction produced no module")
        })
    }

    pub fn set_orbit_module_archived(
        &self,
        module_id: Uuid,
        archived: bool,
    ) -> Result<OrbitModuleDefinition> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            let changed = conn
                .execute(
                    "UPDATE orbit_modules
                     SET archived = ?2, revision = revision + 1, updated_at = ?3
                     WHERE id = ?1",
                    params![
                        module_id.to_string(),
                        bool_to_i64(archived),
                        u64_to_i64(unix_now())?,
                    ],
                )
                .await?;
            anyhow::ensure!(changed == 1, "Orbit module was not found");
            load_orbit_module_async(&conn, module_id)
                .await?
                .context("updated Orbit module disappeared")
        })
    }

    pub fn load_project_orbit_bindings(
        &self,
        project_id: ProjectId,
    ) -> Result<Vec<OrbitProjectBinding>> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            load_project_orbit_bindings_async(&conn, project_id).await
        })
    }

    pub fn set_project_orbit_module_enabled(
        &self,
        project_id: ProjectId,
        module: OrbitModuleId,
        enabled: bool,
    ) -> Result<OrbitProjectBinding> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            let binding = std::cell::RefCell::new(None);
            execute_transaction(&conn, |conn| {
                let binding = &binding;
                Box::pin(async move {
                    binding.replace(Some(
                        set_project_orbit_module_enabled_async(conn, project_id, module, enabled)
                            .await?,
                    ));
                    Ok(())
                })
            })
            .await?;
            binding
                .into_inner()
                .context("Orbit binding transaction produced no binding")
        })
    }

    pub fn load_orbit_records(
        &self,
        project_id: ProjectId,
        module_id: Uuid,
    ) -> Result<Vec<OrbitRecord>> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            load_orbit_records_async(&conn, project_id, module_id).await
        })
    }

    pub fn save_orbit_record(
        &self,
        project_id: ProjectId,
        module_id: Uuid,
        input: OrbitRecordInput,
    ) -> Result<OrbitRecord> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            let record = std::cell::RefCell::new(None);
            execute_transaction(&conn, |conn| {
                let record = &record;
                let input = input.clone();
                Box::pin(async move {
                    let module = active_project_module_async(conn, project_id, module_id).await?;
                    let prepared = prepare_record(&module, project_id, input)?;
                    let stored = upsert_prepared_record_async(conn, prepared, None, None).await?;
                    bump_orbit_data_revision_async(conn, project_id, module_id).await?;
                    record.replace(Some(stored));
                    Ok(())
                })
            })
            .await?;
            record
                .into_inner()
                .context("Orbit record transaction produced no record")
        })
    }

    pub fn delete_orbit_record(
        &self,
        project_id: ProjectId,
        module_id: Uuid,
        record_id: Uuid,
    ) -> Result<()> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            execute_transaction(&conn, |conn| {
                Box::pin(async move {
                    active_project_module_async(conn, project_id, module_id).await?;
                    let changed = conn
                        .execute(
                            "DELETE FROM orbit_records
                             WHERE id = ?1 AND project_id = ?2 AND module_id = ?3",
                            (
                                record_id.to_string(),
                                project_id.0.to_string(),
                                module_id.to_string(),
                            ),
                        )
                        .await?;
                    anyhow::ensure!(changed == 1, "Orbit record was not found");
                    bump_orbit_data_revision_async(conn, project_id, module_id).await?;
                    Ok(())
                })
            })
            .await
        })
    }

    pub fn create_orbit_invocation(
        &self,
        agent_id: Uuid,
        project_id: ProjectId,
        module_id: Uuid,
    ) -> Result<OrbitInvocation> {
        self.create_orbit_invocation_with_id(Uuid::new_v4(), agent_id, project_id, module_id)
    }

    pub fn create_orbit_invocation_with_id(
        &self,
        invocation_id: Uuid,
        agent_id: Uuid,
        project_id: ProjectId,
        module_id: Uuid,
    ) -> Result<OrbitInvocation> {
        self.create_orbit_invocation_with_id_and_ttl(
            invocation_id,
            agent_id,
            project_id,
            module_id,
            ORBIT_INVOCATION_TTL_SECONDS,
        )
    }

    #[doc(hidden)]
    pub fn create_orbit_invocation_with_id_and_ttl(
        &self,
        invocation_id: Uuid,
        agent_id: Uuid,
        project_id: ProjectId,
        module_id: Uuid,
        ttl_seconds: u64,
    ) -> Result<OrbitInvocation> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            create_orbit_invocation_async(
                &conn,
                invocation_id,
                agent_id,
                project_id,
                module_id,
                ttl_seconds,
            )
            .await
        })
    }

    pub fn read_orbit_invocation(
        &self,
        invocation_id: Uuid,
        agent_id: Uuid,
        project_id: ProjectId,
    ) -> Result<OrbitInvocationSnapshot> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            read_orbit_invocation_async(&conn, invocation_id, agent_id, project_id).await
        })
    }

    pub fn apply_orbit_invocation_changes(
        &self,
        invocation_id: Uuid,
        agent_id: Uuid,
        project_id: ProjectId,
        expected_revision: u64,
        upserts: Vec<OrbitRecordInput>,
        delete_record_ids: Vec<Uuid>,
    ) -> Result<OrbitMutationResult> {
        anyhow::ensure!(
            upserts.len() + delete_record_ids.len() <= MAX_RECORD_CHANGES,
            "an Orbit update supports at most {MAX_RECORD_CHANGES} record changes"
        );
        self.rt.block_on(async {
            let conn = self.connect().await?;
            let result = std::cell::RefCell::new(None);
            execute_transaction(&conn, |conn| {
                let result = &result;
                let upserts = upserts.clone();
                let delete_record_ids = delete_record_ids.clone();
                Box::pin(async move {
                    result.replace(Some(
                        apply_orbit_invocation_changes_async(
                            conn,
                            invocation_id,
                            agent_id,
                            project_id,
                            expected_revision,
                            upserts,
                            delete_record_ids,
                        )
                        .await?,
                    ));
                    Ok(())
                })
            })
            .await?;
            result
                .into_inner()
                .context("Orbit mutation transaction produced no result")
        })
    }

    pub fn complete_orbit_invocation(&self, invocation_id: Uuid, agent_id: Uuid) -> Result<()> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            conn.execute(
                "UPDATE orbit_invocations SET completed_at = ?3
                 WHERE id = ?1 AND agent_id = ?2 AND completed_at IS NULL",
                params![
                    invocation_id.to_string(),
                    agent_id.to_string(),
                    u64_to_i64(unix_now())?,
                ],
            )
            .await?;
            Ok(())
        })
    }

    pub fn load_orbit_invocation_update(
        &self,
        invocation_id: Uuid,
        agent_id: Uuid,
    ) -> Result<Option<OrbitInvocationUpdate>> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            let mut rows = conn
                .query(
                    "SELECT i.project_id, i.module_id, m.name,
                            SUM(b.inserted_count), SUM(b.updated_count), SUM(b.deleted_count),
                            MIN(b.created_at),
                            CASE WHEN SUM(CASE WHEN b.undone_at IS NULL THEN 1 ELSE 0 END) = 0
                                 THEN 1 ELSE 0 END
                     FROM orbit_invocations i
                     JOIN orbit_modules m ON m.id = i.module_id
                     JOIN orbit_mutation_batches b ON b.invocation_id = i.id
                     WHERE i.id = ?1 AND i.agent_id = ?2
                     GROUP BY i.project_id, i.module_id, m.name",
                    (invocation_id.to_string(), agent_id.to_string()),
                )
                .await?;
            let Some(row) = rows.next().await? else {
                return Ok(None);
            };
            Ok(Some(OrbitInvocationUpdate {
                invocation_id,
                agent_id,
                project_id: ProjectId(parse_uuid(&row.get::<String>(0)?)?),
                module_id: parse_uuid(&row.get::<String>(1)?)?,
                module_name: row.get(2)?,
                inserted: usize::try_from(row.get::<i64>(3)?)
                    .context("Orbit inserted count is invalid")?,
                updated: usize::try_from(row.get::<i64>(4)?)
                    .context("Orbit updated count is invalid")?,
                deleted: usize::try_from(row.get::<i64>(5)?)
                    .context("Orbit deleted count is invalid")?,
                created_at: i64_to_u64(row.get(6)?)?,
                undone: row.get::<i64>(7)? != 0,
            }))
        })
    }

    pub fn undo_orbit_invocation(&self, invocation_id: Uuid, agent_id: Uuid) -> Result<u64> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            let revision = std::cell::RefCell::new(None);
            execute_transaction(&conn, |conn| {
                let revision = &revision;
                Box::pin(async move {
                    revision.replace(Some(
                        undo_orbit_invocation_async(conn, invocation_id, agent_id).await?,
                    ));
                    Ok(())
                })
            })
            .await?;
            revision
                .into_inner()
                .context("Orbit undo transaction produced no revision")
        })
    }
}

pub(super) async fn save_orbit_module_async(
    conn: &Connection,
    module: &OrbitModuleDefinition,
) -> Result<OrbitModuleDefinition> {
    let mut module = module.clone();
    restore_archived_field_ids_async(conn, &mut module).await?;
    validate_orbit_module(&module)?;
    let mut duplicate = conn
        .query(
            "SELECT id FROM orbit_modules
             WHERE name = ?1 COLLATE NOCASE AND archived = 0 AND id <> ?2 LIMIT 1",
            (module.name.trim(), module.id.to_string()),
        )
        .await?;
    anyhow::ensure!(
        duplicate.next().await?.is_none(),
        "an active Orbit module already uses this name"
    );
    drop(duplicate);

    validate_module_record_keys_async(conn, &module).await?;
    let now = unix_now();
    conn.execute(
        "INSERT INTO orbit_modules
         (id, name, description, view_type, section_key, section_label, agent_job,
          revision, archived, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8, ?9, ?10)
         ON CONFLICT(id) DO UPDATE SET
             name = excluded.name,
             description = excluded.description,
             view_type = excluded.view_type,
             section_key = excluded.section_key,
             section_label = excluded.section_label,
             agent_job = excluded.agent_job,
             revision = orbit_modules.revision + 1,
             archived = excluded.archived,
             updated_at = excluded.updated_at",
        params![
            module.id.to_string(),
            module.name.trim(),
            module.description.trim(),
            module.view_type.storage_label(),
            module.section_key.as_deref(),
            module.section_label.as_deref().map(str::trim),
            module.agent_job.trim(),
            bool_to_i64(module.archived),
            u64_to_i64(now)?,
            u64_to_i64(now)?,
        ],
    )
    .await?;

    let active_ids = module
        .fields
        .iter()
        .map(|field| field.id.to_string())
        .collect::<HashSet<_>>();
    let mut rows = conn
        .query(
            "SELECT id FROM orbit_module_fields WHERE module_id = ?1",
            [module.id.to_string()],
        )
        .await?;
    let mut archived_ids = Vec::new();
    while let Some(row) = rows.next().await? {
        let id: String = row.get(0)?;
        if !active_ids.contains(&id) {
            archived_ids.push(id);
        }
    }
    drop(rows);
    for id in archived_ids {
        conn.execute(
            "UPDATE orbit_module_fields SET archived = 1 WHERE id = ?1",
            [id],
        )
        .await?;
    }
    for field in &module.fields {
        conn.execute(
            "INSERT INTO orbit_module_fields
             (id, module_id, field_key, label, kind, is_primary, sort_order, archived)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET
                 field_key = excluded.field_key,
                 label = excluded.label,
                 kind = excluded.kind,
                 is_primary = excluded.is_primary,
                 sort_order = excluded.sort_order,
                 archived = excluded.archived",
            params![
                field.id.to_string(),
                module.id.to_string(),
                field.key.as_str(),
                field.label.trim(),
                field.kind.storage_label(),
                bool_to_i64(field.primary),
                field.sort_order,
                bool_to_i64(field.archived),
            ],
        )
        .await?;
    }
    recompute_module_record_keys_async(conn, &module).await?;
    load_orbit_module_async(conn, module.id)
        .await?
        .context("saved Orbit module disappeared")
}

async fn restore_archived_field_ids_async(
    conn: &Connection,
    module: &mut OrbitModuleDefinition,
) -> Result<()> {
    let mut rows = conn
        .query(
            "SELECT field_key, id, archived FROM orbit_module_fields WHERE module_id = ?1",
            [module.id.to_string()],
        )
        .await?;
    let mut existing = HashMap::new();
    while let Some(row) = rows.next().await? {
        existing.insert(
            row.get::<String>(0)?,
            (parse_uuid(&row.get::<String>(1)?)?, row.get::<i64>(2)? != 0),
        );
    }
    for field in &mut module.fields {
        let Some((existing_id, archived)) = existing.get(&field.key).copied() else {
            continue;
        };
        if existing_id != field.id {
            anyhow::ensure!(
                archived,
                "an active Orbit field already uses the key '{}'",
                field.key
            );
            field.id = existing_id;
        }
    }
    Ok(())
}

async fn load_orbit_snapshot_async(
    conn: &Connection,
    projects: &[ProjectId],
) -> Result<OrbitSnapshot> {
    let modules = load_orbit_modules_async(conn, true).await?;
    let bindings = load_orbit_bindings_for_projects_async(conn, projects).await?;
    let active_scopes = bindings
        .iter()
        .flat_map(|(project, bindings)| {
            bindings
                .iter()
                .filter_map(move |binding| match binding.module {
                    OrbitModuleId::Custom(module_id) if binding.enabled => {
                        Some((*project, module_id))
                    }
                    _ => None,
                })
        })
        .collect::<HashSet<_>>();
    let records = load_orbit_records_for_scopes_async(conn, &active_scopes).await?;
    Ok(OrbitSnapshot {
        modules,
        bindings,
        records,
    })
}

async fn load_orbit_records_for_scopes_async(
    conn: &Connection,
    active_scopes: &HashSet<(ProjectId, Uuid)>,
) -> Result<HashMap<(ProjectId, Uuid), Vec<OrbitRecord>>> {
    let mut records = active_scopes
        .iter()
        .copied()
        .map(|scope| (scope, Vec::new()))
        .collect::<HashMap<_, _>>();
    if !active_scopes.is_empty() {
        let mut rows = conn
            .query(
                "SELECT id, project_id, module_id, section_value, values_json, record_key,
                        source_agent_id, source_batch_id, created_at, updated_at
                 FROM orbit_records
                 ORDER BY project_id, module_id, COALESCE(section_value, '') COLLATE NOCASE,
                          record_key COLLATE NOCASE",
                (),
            )
            .await?;
        while let Some(row) = rows.next().await? {
            let project_id = ProjectId(parse_uuid(&row.get::<String>(1)?)?);
            let module_id = parse_uuid(&row.get::<String>(2)?)?;
            if !active_scopes.contains(&(project_id, module_id)) {
                continue;
            }
            let record = orbit_record_from_row(&row)?;
            if let Some(scope_records) = records.get_mut(&(record.project_id, record.module_id)) {
                scope_records.push(record);
            }
        }
    }
    Ok(records)
}

pub(super) async fn load_orbit_modules_async(
    conn: &Connection,
    include_archived: bool,
) -> Result<Vec<OrbitModuleDefinition>> {
    let sql = if include_archived {
        "SELECT id, name, description, view_type, section_key, section_label,
                agent_job, revision, archived, created_at, updated_at
         FROM orbit_modules ORDER BY archived ASC, name COLLATE NOCASE"
    } else {
        "SELECT id, name, description, view_type, section_key, section_label,
                agent_job, revision, archived, created_at, updated_at
         FROM orbit_modules WHERE archived = 0 ORDER BY name COLLATE NOCASE"
    };
    let mut rows = conn.query(sql, ()).await?;
    let mut modules = Vec::new();
    let mut module_indexes = HashMap::new();
    while let Some(row) = rows.next().await? {
        let module = orbit_module_from_row(&row)?;
        module_indexes.insert(module.id, modules.len());
        modules.push(module);
    }
    drop(rows);

    let mut fields = conn
        .query(
            "SELECT module_id, id, field_key, label, kind, is_primary, sort_order, archived
             FROM orbit_module_fields
             ORDER BY module_id, archived ASC, sort_order ASC, label COLLATE NOCASE",
            (),
        )
        .await?;
    while let Some(row) = fields.next().await? {
        let module_id = parse_uuid(&row.get::<String>(0)?)?;
        let Some(index) = module_indexes.get(&module_id).copied() else {
            continue;
        };
        modules[index].fields.push(OrbitFieldDefinition {
            id: parse_uuid(&row.get::<String>(1)?)?,
            key: row.get(2)?,
            label: row.get(3)?,
            kind: OrbitFieldKind::from_storage_label(&row.get::<String>(4)?)?,
            primary: row.get::<i64>(5)? != 0,
            sort_order: row.get(6)?,
            archived: row.get::<i64>(7)? != 0,
        });
    }
    Ok(modules)
}

fn orbit_module_from_row(row: &turso::Row) -> Result<OrbitModuleDefinition> {
    Ok(OrbitModuleDefinition {
        id: parse_uuid(&row.get::<String>(0)?)?,
        name: row.get(1)?,
        description: row.get(2)?,
        view_type: OrbitViewType::from_storage_label(&row.get::<String>(3)?)?,
        section_key: opt_text(row, 4)?,
        section_label: opt_text(row, 5)?,
        agent_job: row.get(6)?,
        revision: i64_to_u64(row.get(7)?)?,
        archived: row.get::<i64>(8)? != 0,
        fields: Vec::new(),
        created_at: i64_to_u64(row.get(9)?)?,
        updated_at: i64_to_u64(row.get(10)?)?,
    })
}

async fn load_orbit_fields_async(
    conn: &Connection,
    module: &mut OrbitModuleDefinition,
) -> Result<()> {
    let mut fields = conn
        .query(
            "SELECT id, field_key, label, kind, is_primary, sort_order, archived
             FROM orbit_module_fields WHERE module_id = ?1
             ORDER BY archived ASC, sort_order ASC, label COLLATE NOCASE",
            [module.id.to_string()],
        )
        .await?;
    while let Some(row) = fields.next().await? {
        module.fields.push(OrbitFieldDefinition {
            id: parse_uuid(&row.get::<String>(0)?)?,
            key: row.get(1)?,
            label: row.get(2)?,
            kind: OrbitFieldKind::from_storage_label(&row.get::<String>(3)?)?,
            primary: row.get::<i64>(4)? != 0,
            sort_order: row.get(5)?,
            archived: row.get::<i64>(6)? != 0,
        });
    }
    Ok(())
}

async fn load_orbit_module_async(
    conn: &Connection,
    id: Uuid,
) -> Result<Option<OrbitModuleDefinition>> {
    let mut rows = conn
        .query(
            "SELECT id, name, description, view_type, section_key, section_label,
                    agent_job, revision, archived, created_at, updated_at
             FROM orbit_modules WHERE id = ?1",
            [id.to_string()],
        )
        .await?;
    let Some(row) = rows.next().await? else {
        return Ok(None);
    };
    let mut module = orbit_module_from_row(&row)?;
    drop(rows);
    load_orbit_fields_async(conn, &mut module).await?;
    Ok(Some(module))
}

async fn validate_module_record_keys_async(
    conn: &Connection,
    module: &OrbitModuleDefinition,
) -> Result<()> {
    let mut rows = conn
        .query(
            "SELECT project_id, section_value, values_json FROM orbit_records WHERE module_id = ?1",
            [module.id.to_string()],
        )
        .await?;
    let mut keys = HashSet::new();
    while let Some(row) = rows.next().await? {
        let project_id: String = row.get(0)?;
        let section = opt_text(&row, 1)?;
        let values: BTreeMap<String, serde_json::Value> =
            serde_json::from_str(&row.get::<String>(2)?)?;
        validate_orbit_values(module, section.as_deref(), &values, true)?;
        let key = orbit_record_key(module, section.as_deref(), &values)?;
        anyhow::ensure!(
            keys.insert((project_id, key)),
            "this schema would make two existing records share the same identity"
        );
    }
    Ok(())
}

async fn recompute_module_record_keys_async(
    conn: &Connection,
    module: &OrbitModuleDefinition,
) -> Result<()> {
    let mut rows = conn
        .query(
            "SELECT id, section_value, values_json FROM orbit_records WHERE module_id = ?1",
            [module.id.to_string()],
        )
        .await?;
    let mut updates = Vec::new();
    while let Some(row) = rows.next().await? {
        let id: String = row.get(0)?;
        let section = opt_text(&row, 1)?;
        let values: BTreeMap<String, serde_json::Value> =
            serde_json::from_str(&row.get::<String>(2)?)?;
        updates.push((id, orbit_record_key(module, section.as_deref(), &values)?));
    }
    drop(rows);
    for (id, key) in updates {
        conn.execute(
            "UPDATE orbit_records SET record_key = ?2 WHERE id = ?1",
            (id, key),
        )
        .await?;
    }
    Ok(())
}

pub(super) async fn load_project_orbit_bindings_async(
    conn: &Connection,
    project_id: ProjectId,
) -> Result<Vec<OrbitProjectBinding>> {
    Ok(load_orbit_bindings_for_projects_async(conn, &[project_id])
        .await?
        .remove(&project_id)
        .unwrap_or_else(|| default_orbit_builtin_bindings(project_id)))
}

pub(super) async fn load_orbit_bindings_for_projects_async(
    conn: &Connection,
    projects: &[ProjectId],
) -> Result<HashMap<ProjectId, Vec<OrbitProjectBinding>>> {
    let project_ids = projects.iter().copied().collect::<HashSet<_>>();
    let mut stored = projects
        .iter()
        .copied()
        .map(|project| (project, HashMap::new()))
        .collect::<HashMap<_, HashMap<OrbitModuleId, OrbitProjectBinding>>>();
    let mut rows = conn
        .query(
            "SELECT project_id, module_key, enabled, sort_order, data_revision, created_at, updated_at
             FROM orbit_project_modules ORDER BY project_id, sort_order ASC",
            (),
        )
        .await?;
    while let Some(row) = rows.next().await? {
        let project_id = ProjectId(parse_uuid(&row.get::<String>(0)?)?);
        if !project_ids.contains(&project_id) {
            continue;
        }
        let module = OrbitModuleId::from_storage_key(&row.get::<String>(1)?)?;
        let binding = OrbitProjectBinding {
            project_id,
            module,
            enabled: row.get::<i64>(2)? != 0,
            sort_order: row.get(3)?,
            data_revision: i64_to_u64(row.get(4)?)?,
            created_at: i64_to_u64(row.get(5)?)?,
            updated_at: i64_to_u64(row.get(6)?)?,
        };
        stored
            .entry(project_id)
            .or_default()
            .insert(module, binding);
    }
    let mut result = HashMap::with_capacity(projects.len());
    for project_id in projects.iter().copied() {
        let mut project_stored = stored.remove(&project_id).unwrap_or_default();
        let mut project_bindings = default_orbit_builtin_bindings(project_id)
            .into_iter()
            .map(|default| project_stored.remove(&default.module).unwrap_or(default))
            .collect::<Vec<_>>();
        let mut custom = project_stored.into_values().collect::<Vec<_>>();
        custom.sort_by_key(|binding| binding.sort_order);
        project_bindings.extend(custom);
        result.insert(project_id, project_bindings);
    }
    Ok(result)
}

fn default_orbit_builtin_bindings(project_id: ProjectId) -> Vec<OrbitProjectBinding> {
    let now = unix_now();
    [OrbitBuiltin::Environment, OrbitBuiltin::Integrations]
        .into_iter()
        .enumerate()
        .map(|(index, builtin)| OrbitProjectBinding {
            project_id,
            module: OrbitModuleId::Builtin(builtin),
            enabled: true,
            sort_order: index as i64,
            data_revision: 0,
            created_at: now,
            updated_at: now,
        })
        .collect()
}

async fn set_project_orbit_module_enabled_async(
    conn: &Connection,
    project_id: ProjectId,
    module: OrbitModuleId,
    enabled: bool,
) -> Result<OrbitProjectBinding> {
    ensure_project_exists_async(conn, project_id).await?;
    if let OrbitModuleId::Custom(module_id) = module {
        let definition = load_orbit_module_async(conn, module_id)
            .await?
            .context("Orbit module was not found")?;
        anyhow::ensure!(
            !definition.archived,
            "archived Orbit modules cannot be added to projects"
        );
    }
    let key = module.storage_key();
    let now = unix_now();
    let mut max_rows = conn
        .query(
            "SELECT COALESCE(MAX(sort_order), 1) FROM orbit_project_modules WHERE project_id = ?1",
            [project_id.0.to_string()],
        )
        .await?;
    let max_order: i64 = max_rows
        .next()
        .await?
        .map(|row| row.get(0))
        .transpose()?
        .unwrap_or(1);
    drop(max_rows);
    let default_order = match module {
        OrbitModuleId::Builtin(OrbitBuiltin::Environment) => 0,
        OrbitModuleId::Builtin(OrbitBuiltin::Integrations) => 1,
        OrbitModuleId::Custom(_) => max_order.saturating_add(1),
    };
    conn.execute(
        "INSERT INTO orbit_project_modules
         (project_id, module_key, enabled, sort_order, data_revision, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6)
         ON CONFLICT(project_id, module_key) DO UPDATE SET
             enabled = excluded.enabled,
             updated_at = excluded.updated_at",
        params![
            project_id.0.to_string(),
            key.as_str(),
            bool_to_i64(enabled),
            default_order,
            u64_to_i64(now)?,
            u64_to_i64(now)?,
        ],
    )
    .await?;
    load_binding_async(conn, project_id, module)
        .await?
        .context("saved Orbit binding disappeared")
}

async fn load_binding_async(
    conn: &Connection,
    project_id: ProjectId,
    module: OrbitModuleId,
) -> Result<Option<OrbitProjectBinding>> {
    let mut rows = conn
        .query(
            "SELECT enabled, sort_order, data_revision, created_at, updated_at
             FROM orbit_project_modules WHERE project_id = ?1 AND module_key = ?2",
            (project_id.0.to_string(), module.storage_key()),
        )
        .await?;
    let Some(row) = rows.next().await? else {
        return Ok(None);
    };
    Ok(Some(OrbitProjectBinding {
        project_id,
        module,
        enabled: row.get::<i64>(0)? != 0,
        sort_order: row.get(1)?,
        data_revision: i64_to_u64(row.get(2)?)?,
        created_at: i64_to_u64(row.get(3)?)?,
        updated_at: i64_to_u64(row.get(4)?)?,
    }))
}

async fn active_project_module_async(
    conn: &Connection,
    project_id: ProjectId,
    module_id: Uuid,
) -> Result<OrbitModuleDefinition> {
    let module = load_orbit_module_async(conn, module_id)
        .await?
        .context("Orbit module was not found")?;
    anyhow::ensure!(!module.archived, "Orbit module is archived");
    let binding = load_binding_async(conn, project_id, OrbitModuleId::Custom(module_id))
        .await?
        .context("Orbit module is not added to this project")?;
    anyhow::ensure!(binding.enabled, "Orbit module is removed from this project");
    Ok(module)
}

async fn ensure_project_exists_async(conn: &Connection, project_id: ProjectId) -> Result<()> {
    let mut rows = conn
        .query(
            "SELECT 1 FROM projects WHERE id = ?1",
            [project_id.0.to_string()],
        )
        .await?;
    anyhow::ensure!(rows.next().await?.is_some(), "project was not found");
    Ok(())
}

pub(super) async fn load_orbit_records_async(
    conn: &Connection,
    project_id: ProjectId,
    module_id: Uuid,
) -> Result<Vec<OrbitRecord>> {
    let mut rows = conn
        .query(
            "SELECT id, project_id, module_id, section_value, values_json, record_key,
                    source_agent_id, source_batch_id, created_at, updated_at
             FROM orbit_records WHERE project_id = ?1 AND module_id = ?2
             ORDER BY COALESCE(section_value, '') COLLATE NOCASE, record_key COLLATE NOCASE",
            (project_id.0.to_string(), module_id.to_string()),
        )
        .await?;
    let mut records = Vec::new();
    while let Some(row) = rows.next().await? {
        records.push(orbit_record_from_row(&row)?);
    }
    Ok(records)
}

pub(super) async fn load_all_orbit_records_async(conn: &Connection) -> Result<Vec<OrbitRecord>> {
    let mut rows = conn
        .query(
            "SELECT id, project_id, module_id, section_value, values_json, record_key,
                    source_agent_id, source_batch_id, created_at, updated_at
             FROM orbit_records
             ORDER BY project_id, module_id, COALESCE(section_value, '') COLLATE NOCASE,
                      record_key COLLATE NOCASE",
            (),
        )
        .await?;
    let mut records = Vec::new();
    while let Some(row) = rows.next().await? {
        records.push(orbit_record_from_row(&row)?);
    }
    Ok(records)
}

fn orbit_record_from_row(row: &turso::Row) -> Result<OrbitRecord> {
    Ok(OrbitRecord {
        id: parse_uuid(&row.get::<String>(0)?)?,
        project_id: ProjectId(parse_uuid(&row.get::<String>(1)?)?),
        module_id: parse_uuid(&row.get::<String>(2)?)?,
        section: opt_text(row, 3)?,
        values: serde_json::from_str(&row.get::<String>(4)?)?,
        record_key: row.get(5)?,
        source_agent_id: opt_text(row, 6)?
            .map(|value| parse_uuid(&value))
            .transpose()?,
        source_batch_id: opt_text(row, 7)?
            .map(|value| parse_uuid(&value))
            .transpose()?,
        created_at: i64_to_u64(row.get(8)?)?,
        updated_at: i64_to_u64(row.get(9)?)?,
    })
}

fn validate_orbit_values(
    module: &OrbitModuleDefinition,
    section: Option<&str>,
    values: &BTreeMap<String, serde_json::Value>,
    allow_archived: bool,
) -> Result<()> {
    if module.section_key.is_some() {
        let section = section.map(str::trim).unwrap_or_default();
        anyhow::ensure!(
            !section.is_empty(),
            "{} is required",
            module.section_label.as_deref().unwrap_or("Section")
        );
        anyhow::ensure!(
            section.chars().count() <= MAX_SHORT_TEXT_CHARS,
            "section value is too long"
        );
    }
    let fields = module
        .fields
        .iter()
        .map(|field| (field.key.as_str(), field))
        .collect::<HashMap<_, _>>();
    for (key, value) in values {
        let Some(field) = fields.get(key.as_str()) else {
            if allow_archived {
                continue;
            }
            return Err(anyhow!("unknown Orbit field: {key}"));
        };
        anyhow::ensure!(
            allow_archived || !field.archived,
            "Orbit field '{key}' is archived"
        );
        match field.kind {
            OrbitFieldKind::ShortText => {
                let text = value
                    .as_str()
                    .ok_or_else(|| anyhow!("{key} must be text"))?;
                anyhow::ensure!(
                    text.chars().count() <= MAX_SHORT_TEXT_CHARS,
                    "{key} is too long"
                );
            }
            OrbitFieldKind::LongText => {
                let text = value
                    .as_str()
                    .ok_or_else(|| anyhow!("{key} must be text"))?;
                anyhow::ensure!(
                    text.chars().count() <= MAX_LONG_TEXT_CHARS,
                    "{key} is too long"
                );
            }
            OrbitFieldKind::List => {
                let items = value
                    .as_array()
                    .ok_or_else(|| anyhow!("{key} must be a list"))?;
                anyhow::ensure!(items.len() <= MAX_LIST_ITEMS, "{key} has too many items");
                for item in items {
                    let text = item
                        .as_str()
                        .ok_or_else(|| anyhow!("{key} list items must be text"))?;
                    anyhow::ensure!(
                        text.chars().count() <= MAX_LIST_ITEM_CHARS,
                        "{key} contains an item that is too long"
                    );
                }
            }
        }
    }
    let primary = module
        .fields
        .iter()
        .find(|field| field.primary && !field.archived)
        .context("Orbit module has no primary field")?;
    let primary_value = values
        .get(&primary.key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    anyhow::ensure!(!primary_value.is_empty(), "{} is required", primary.label);
    Ok(())
}

fn orbit_record_key(
    module: &OrbitModuleDefinition,
    section: Option<&str>,
    values: &BTreeMap<String, serde_json::Value>,
) -> Result<String> {
    let primary = module
        .fields
        .iter()
        .find(|field| field.primary && !field.archived)
        .context("Orbit module has no primary field")?;
    let primary = values
        .get(&primary.key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_lowercase();
    let section = if module.section_key.is_some() {
        section.unwrap_or_default().trim().to_lowercase()
    } else {
        String::new()
    };
    Ok(format!("{section}\u{0}{primary}"))
}

fn prepare_record(
    module: &OrbitModuleDefinition,
    project_id: ProjectId,
    mut input: OrbitRecordInput,
) -> Result<OrbitRecord> {
    if module.section_key.is_none() {
        input.section = None;
    } else {
        input.section = input.section.map(|section| section.trim().to_string());
    }
    for value in input.values.values_mut() {
        if let Some(text) = value.as_str() {
            *value = serde_json::Value::String(text.trim().to_string());
        }
    }
    validate_orbit_values(module, input.section.as_deref(), &input.values, false)?;
    let record_key = orbit_record_key(module, input.section.as_deref(), &input.values)?;
    let now = unix_now();
    Ok(OrbitRecord {
        id: input.id.unwrap_or_else(Uuid::new_v4),
        project_id,
        module_id: module.id,
        section: input.section,
        values: input.values,
        record_key,
        source_agent_id: None,
        source_batch_id: None,
        created_at: now,
        updated_at: now,
    })
}

async fn upsert_prepared_record_async(
    conn: &Connection,
    mut record: OrbitRecord,
    source_agent_id: Option<Uuid>,
    source_batch_id: Option<Uuid>,
) -> Result<OrbitRecord> {
    if let Some(existing) = load_record_by_key_async(
        conn,
        record.project_id,
        record.module_id,
        &record.record_key,
    )
    .await?
    {
        if record.id != existing.id {
            record.id = existing.id;
        }
        for (key, value) in existing.values {
            record.values.entry(key).or_insert(value);
        }
        record.created_at = existing.created_at;
    }
    if let Some(existing) = load_record_by_id_async(conn, record.id).await? {
        anyhow::ensure!(
            existing.project_id == record.project_id && existing.module_id == record.module_id,
            "Orbit record belongs to another project or module"
        );
        for (key, value) in existing.values {
            record.values.entry(key).or_insert(value);
        }
        record.created_at = existing.created_at;
    }
    record.source_agent_id = source_agent_id;
    record.source_batch_id = source_batch_id;
    record.updated_at = unix_now();
    conn.execute(
        "INSERT INTO orbit_records
         (id, project_id, module_id, section_value, values_json, record_key,
          source_agent_id, source_batch_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(id) DO UPDATE SET
             section_value = excluded.section_value,
             values_json = excluded.values_json,
             record_key = excluded.record_key,
             source_agent_id = excluded.source_agent_id,
             source_batch_id = excluded.source_batch_id,
             updated_at = excluded.updated_at",
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
    load_record_by_id_async(conn, record.id)
        .await?
        .context("saved Orbit record disappeared")
}

async fn load_record_by_id_async(conn: &Connection, id: Uuid) -> Result<Option<OrbitRecord>> {
    let mut rows = conn
        .query(
            "SELECT id, project_id, module_id, section_value, values_json, record_key,
                    source_agent_id, source_batch_id, created_at, updated_at
             FROM orbit_records WHERE id = ?1",
            [id.to_string()],
        )
        .await?;
    rows.next()
        .await?
        .map(|row| orbit_record_from_row(&row))
        .transpose()
}

async fn load_record_by_key_async(
    conn: &Connection,
    project_id: ProjectId,
    module_id: Uuid,
    key: &str,
) -> Result<Option<OrbitRecord>> {
    let mut rows = conn
        .query(
            "SELECT id, project_id, module_id, section_value, values_json, record_key,
                    source_agent_id, source_batch_id, created_at, updated_at
             FROM orbit_records WHERE project_id = ?1 AND module_id = ?2 AND record_key = ?3",
            (project_id.0.to_string(), module_id.to_string(), key),
        )
        .await?;
    rows.next()
        .await?
        .map(|row| orbit_record_from_row(&row))
        .transpose()
}

async fn bump_orbit_data_revision_async(
    conn: &Connection,
    project_id: ProjectId,
    module_id: Uuid,
) -> Result<u64> {
    let now = unix_now();
    let changed = conn
        .execute(
            "UPDATE orbit_project_modules
             SET data_revision = data_revision + 1, updated_at = ?3
             WHERE project_id = ?1 AND module_key = ?2",
            params![
                project_id.0.to_string(),
                OrbitModuleId::Custom(module_id).storage_key(),
                u64_to_i64(now)?,
            ],
        )
        .await?;
    anyhow::ensure!(changed == 1, "Orbit project binding was not found");
    let binding = load_binding_async(conn, project_id, OrbitModuleId::Custom(module_id))
        .await?
        .context("Orbit project binding disappeared")?;
    Ok(binding.data_revision)
}

async fn create_orbit_invocation_async(
    conn: &Connection,
    invocation_id: Uuid,
    agent_id: Uuid,
    project_id: ProjectId,
    module_id: Uuid,
    ttl_seconds: u64,
) -> Result<OrbitInvocation> {
    let module = active_project_module_async(conn, project_id, module_id).await?;
    let mut agents = conn
        .query(
            "SELECT project_id FROM agents WHERE id = ?1",
            [agent_id.to_string()],
        )
        .await?;
    let Some(agent) = agents.next().await? else {
        return Err(anyhow!("Orbit invocation agent was not found"));
    };
    anyhow::ensure!(
        ProjectId(parse_uuid(&agent.get::<String>(0)?)?) == project_id,
        "Orbit invocation agent belongs to another project"
    );
    drop(agents);
    let binding = load_binding_async(conn, project_id, OrbitModuleId::Custom(module_id))
        .await?
        .context("Orbit module is not added to this project")?;
    let now = unix_now();
    let invocation = OrbitInvocation {
        id: invocation_id,
        agent_id,
        project_id,
        module_id,
        module_revision: module.revision,
        data_revision: binding.data_revision,
        expires_at: now.saturating_add(ttl_seconds),
        completed_at: None,
        created_at: now,
    };
    conn.execute(
        "INSERT INTO orbit_invocations
         (id, agent_id, project_id, module_id, module_revision, data_revision,
          expires_at, completed_at, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, ?8)",
        params![
            invocation.id.to_string(),
            invocation.agent_id.to_string(),
            invocation.project_id.0.to_string(),
            invocation.module_id.to_string(),
            u64_to_i64(invocation.module_revision)?,
            u64_to_i64(invocation.data_revision)?,
            u64_to_i64(invocation.expires_at)?,
            u64_to_i64(invocation.created_at)?,
        ],
    )
    .await?;
    Ok(invocation)
}

async fn load_invocation_async(
    conn: &Connection,
    invocation_id: Uuid,
) -> Result<Option<OrbitInvocation>> {
    let mut rows = conn
        .query(
            "SELECT id, agent_id, project_id, module_id, module_revision, data_revision,
                    expires_at, completed_at, created_at
             FROM orbit_invocations WHERE id = ?1",
            [invocation_id.to_string()],
        )
        .await?;
    let Some(row) = rows.next().await? else {
        return Ok(None);
    };
    Ok(Some(OrbitInvocation {
        id: parse_uuid(&row.get::<String>(0)?)?,
        agent_id: parse_uuid(&row.get::<String>(1)?)?,
        project_id: ProjectId(parse_uuid(&row.get::<String>(2)?)?),
        module_id: parse_uuid(&row.get::<String>(3)?)?,
        module_revision: i64_to_u64(row.get(4)?)?,
        data_revision: i64_to_u64(row.get(5)?)?,
        expires_at: i64_to_u64(row.get(6)?)?,
        completed_at: opt_i64(&row, 7)?.map(i64_to_u64).transpose()?,
        created_at: i64_to_u64(row.get(8)?)?,
    }))
}

pub(super) async fn load_all_orbit_invocations_async(
    conn: &Connection,
) -> Result<Vec<OrbitInvocation>> {
    let mut rows = conn
        .query(
            "SELECT id, agent_id, project_id, module_id, module_revision, data_revision,
                    expires_at, completed_at, created_at
             FROM orbit_invocations
             ORDER BY created_at ASC, id ASC",
            (),
        )
        .await?;
    let mut invocations = Vec::new();
    while let Some(row) = rows.next().await? {
        invocations.push(OrbitInvocation {
            id: parse_uuid(&row.get::<String>(0)?)?,
            agent_id: parse_uuid(&row.get::<String>(1)?)?,
            project_id: ProjectId(parse_uuid(&row.get::<String>(2)?)?),
            module_id: parse_uuid(&row.get::<String>(3)?)?,
            module_revision: i64_to_u64(row.get(4)?)?,
            data_revision: i64_to_u64(row.get(5)?)?,
            expires_at: i64_to_u64(row.get(6)?)?,
            completed_at: opt_i64(&row, 7)?.map(i64_to_u64).transpose()?,
            created_at: i64_to_u64(row.get(8)?)?,
        });
    }
    Ok(invocations)
}

pub(super) async fn load_all_orbit_mutation_batches_async(
    conn: &Connection,
) -> Result<Vec<OrbitMutationBatch>> {
    let mut rows = conn
        .query(
            "SELECT id, invocation_id, agent_id, project_id, module_id,
                    revision_before, revision_after, before_json, after_json,
                    inserted_count, updated_count, deleted_count, created_at, undone_at
             FROM orbit_mutation_batches
             ORDER BY created_at ASC, id ASC",
            (),
        )
        .await?;
    let mut batches = Vec::new();
    while let Some(row) = rows.next().await? {
        batches.push(OrbitMutationBatch {
            id: parse_uuid(&row.get::<String>(0)?)?,
            invocation_id: parse_uuid(&row.get::<String>(1)?)?,
            agent_id: parse_uuid(&row.get::<String>(2)?)?,
            project_id: ProjectId(parse_uuid(&row.get::<String>(3)?)?),
            module_id: parse_uuid(&row.get::<String>(4)?)?,
            revision_before: i64_to_u64(row.get(5)?)?,
            revision_after: i64_to_u64(row.get(6)?)?,
            before: serde_json::from_str(&row.get::<String>(7)?)?,
            after: serde_json::from_str(&row.get::<String>(8)?)?,
            inserted: usize::try_from(row.get::<i64>(9)?)?,
            updated: usize::try_from(row.get::<i64>(10)?)?,
            deleted: usize::try_from(row.get::<i64>(11)?)?,
            created_at: i64_to_u64(row.get(12)?)?,
            undone_at: opt_i64(&row, 13)?.map(i64_to_u64).transpose()?,
        });
    }
    Ok(batches)
}

async fn read_orbit_invocation_async(
    conn: &Connection,
    invocation_id: Uuid,
    agent_id: Uuid,
    project_id: ProjectId,
) -> Result<OrbitInvocationSnapshot> {
    let invocation = load_invocation_async(conn, invocation_id)
        .await?
        .context("Orbit invocation was not found")?;
    anyhow::ensure!(
        invocation.agent_id == agent_id,
        "Orbit invocation belongs to another agent"
    );
    anyhow::ensure!(
        invocation.project_id == project_id,
        "Orbit invocation belongs to another project"
    );
    anyhow::ensure!(
        invocation.completed_at.is_none(),
        "Orbit invocation is already complete"
    );
    anyhow::ensure!(
        invocation.expires_at > unix_now(),
        "Orbit invocation has expired"
    );
    let module = active_project_module_async(conn, project_id, invocation.module_id).await?;
    anyhow::ensure!(
        module.revision == invocation.module_revision,
        "Orbit module changed after this invocation started; invoke it again"
    );
    let binding = load_binding_async(
        conn,
        project_id,
        OrbitModuleId::Custom(invocation.module_id),
    )
    .await?
    .context("Orbit project binding was not found")?;
    Ok(OrbitInvocationSnapshot {
        records: load_orbit_records_async(conn, project_id, invocation.module_id).await?,
        invocation,
        module,
        data_revision: binding.data_revision,
    })
}

async fn apply_orbit_invocation_changes_async(
    conn: &Connection,
    invocation_id: Uuid,
    agent_id: Uuid,
    project_id: ProjectId,
    expected_revision: u64,
    upserts: Vec<OrbitRecordInput>,
    delete_record_ids: Vec<Uuid>,
) -> Result<OrbitMutationResult> {
    anyhow::ensure!(
        !upserts.is_empty() || !delete_record_ids.is_empty(),
        "Orbit update has no changes"
    );
    let snapshot = read_orbit_invocation_async(conn, invocation_id, agent_id, project_id).await?;
    anyhow::ensure!(
        snapshot.data_revision == expected_revision,
        "Orbit records changed since they were read (expected revision {expected_revision}, current revision {}); call orbit_read again",
        snapshot.data_revision
    );
    let before = snapshot.records;
    let existing_ids = before
        .iter()
        .map(|record| record.id)
        .collect::<HashSet<_>>();
    let mut delete_ids = HashSet::new();
    for id in delete_record_ids {
        anyhow::ensure!(
            existing_ids.contains(&id),
            "Orbit delete target was not found in this module"
        );
        anyhow::ensure!(delete_ids.insert(id), "Orbit delete target is duplicated");
    }
    let batch_id = Uuid::new_v4();
    let mut inserted = 0;
    let mut updated = 0;
    let mut seen_input_keys = HashSet::new();
    for input in upserts {
        let prepared = prepare_record(&snapshot.module, project_id, input)?;
        anyhow::ensure!(
            seen_input_keys.insert(prepared.record_key.clone()),
            "Orbit update contains duplicate records"
        );
        let existed = load_record_by_id_async(conn, prepared.id).await?.is_some()
            || load_record_by_key_async(conn, project_id, snapshot.module.id, &prepared.record_key)
                .await?
                .is_some();
        upsert_prepared_record_async(conn, prepared, Some(agent_id), Some(batch_id)).await?;
        if existed {
            updated += 1;
        } else {
            inserted += 1;
        }
    }
    for id in &delete_ids {
        conn.execute(
            "DELETE FROM orbit_records WHERE id = ?1 AND project_id = ?2 AND module_id = ?3",
            (
                id.to_string(),
                project_id.0.to_string(),
                snapshot.module.id.to_string(),
            ),
        )
        .await?;
    }
    let revision_after =
        bump_orbit_data_revision_async(conn, project_id, snapshot.module.id).await?;
    let after = load_orbit_records_async(conn, project_id, snapshot.module.id).await?;
    conn.execute(
        "INSERT INTO orbit_mutation_batches
         (id, invocation_id, agent_id, project_id, module_id, revision_before,
          revision_after, before_json, after_json, inserted_count, updated_count,
          deleted_count, created_at, undone_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, NULL)",
        params![
            batch_id.to_string(),
            invocation_id.to_string(),
            agent_id.to_string(),
            project_id.0.to_string(),
            snapshot.module.id.to_string(),
            u64_to_i64(snapshot.data_revision)?,
            u64_to_i64(revision_after)?,
            serde_json::to_string(&before)?,
            serde_json::to_string(&after)?,
            inserted as i64,
            updated as i64,
            delete_ids.len() as i64,
            u64_to_i64(unix_now())?,
        ],
    )
    .await?;
    Ok(OrbitMutationResult {
        invocation_id,
        batch_id,
        inserted,
        updated,
        deleted: delete_ids.len(),
        data_revision: revision_after,
    })
}

async fn undo_orbit_invocation_async(
    conn: &Connection,
    invocation_id: Uuid,
    agent_id: Uuid,
) -> Result<u64> {
    let invocation = load_invocation_async(conn, invocation_id)
        .await?
        .context("Orbit invocation was not found")?;
    anyhow::ensure!(
        invocation.agent_id == agent_id,
        "Orbit invocation belongs to another agent"
    );
    let mut rows = conn
        .query(
            "SELECT id, invocation_id, agent_id, project_id, module_id,
                    revision_before, revision_after, before_json, after_json,
                    inserted_count, updated_count, deleted_count, created_at, undone_at
             FROM orbit_mutation_batches
             WHERE invocation_id = ?1 AND undone_at IS NULL
             ORDER BY revision_after DESC, created_at DESC",
            [invocation_id.to_string()],
        )
        .await?;
    let mut batches = Vec::new();
    while let Some(row) = rows.next().await? {
        batches.push(OrbitMutationBatch {
            id: parse_uuid(&row.get::<String>(0)?)?,
            invocation_id: parse_uuid(&row.get::<String>(1)?)?,
            agent_id: parse_uuid(&row.get::<String>(2)?)?,
            project_id: ProjectId(parse_uuid(&row.get::<String>(3)?)?),
            module_id: parse_uuid(&row.get::<String>(4)?)?,
            revision_before: i64_to_u64(row.get(5)?)?,
            revision_after: i64_to_u64(row.get(6)?)?,
            before: serde_json::from_str(&row.get::<String>(7)?)?,
            after: serde_json::from_str(&row.get::<String>(8)?)?,
            inserted: usize::try_from(row.get::<i64>(9)?)?,
            updated: usize::try_from(row.get::<i64>(10)?)?,
            deleted: usize::try_from(row.get::<i64>(11)?)?,
            created_at: i64_to_u64(row.get(12)?)?,
            undone_at: opt_i64(&row, 13)?.map(i64_to_u64).transpose()?,
        });
    }
    anyhow::ensure!(
        !batches.is_empty(),
        "this Orbit update is already undone or made no changes"
    );
    active_project_module_async(conn, invocation.project_id, invocation.module_id).await?;
    let mut current = load_orbit_records_async(conn, invocation.project_id, invocation.module_id)
        .await?
        .into_iter()
        .map(|record| (record.id, record))
        .collect::<HashMap<_, _>>();

    for batch in &batches {
        let before = batch
            .before
            .iter()
            .cloned()
            .map(|record| (record.id, record))
            .collect::<HashMap<_, _>>();
        let after = batch
            .after
            .iter()
            .cloned()
            .map(|record| (record.id, record))
            .collect::<HashMap<_, _>>();
        let touched = before
            .keys()
            .chain(after.keys())
            .copied()
            .collect::<HashSet<_>>();

        for record_id in touched {
            let before_record = before.get(&record_id);
            let after_record = after.get(&record_id);
            if before_record == after_record {
                continue;
            }
            anyhow::ensure!(
                current.get(&record_id) == after_record,
                "an Orbit record changed after this update, so it cannot be undone safely"
            );

            match before_record {
                Some(record) => {
                    if current.values().any(|candidate| {
                        candidate.id != record.id && candidate.record_key == record.record_key
                    }) {
                        return Err(anyhow!(
                            "an Orbit record identity was reused after this update, so it cannot be undone safely"
                        ));
                    }
                    write_orbit_record_snapshot_async(conn, record).await?;
                    current.insert(record.id, record.clone());
                }
                None => {
                    conn.execute(
                        "DELETE FROM orbit_records
                         WHERE id = ?1 AND project_id = ?2 AND module_id = ?3",
                        (
                            record_id.to_string(),
                            invocation.project_id.0.to_string(),
                            invocation.module_id.to_string(),
                        ),
                    )
                    .await?;
                    current.remove(&record_id);
                }
            }
        }
    }
    let revision =
        bump_orbit_data_revision_async(conn, invocation.project_id, invocation.module_id).await?;
    let now = u64_to_i64(unix_now())?;
    for batch in batches {
        conn.execute(
            "UPDATE orbit_mutation_batches SET undone_at = ?2 WHERE id = ?1",
            (batch.id.to_string(), now),
        )
        .await?;
    }
    Ok(revision)
}

async fn write_orbit_record_snapshot_async(conn: &Connection, record: &OrbitRecord) -> Result<()> {
    conn.execute(
        "INSERT INTO orbit_records
         (id, project_id, module_id, section_value, values_json, record_key,
          source_agent_id, source_batch_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(id) DO UPDATE SET
             section_value = excluded.section_value,
             values_json = excluded.values_json,
             record_key = excluded.record_key,
             source_agent_id = excluded.source_agent_id,
             source_batch_id = excluded.source_batch_id,
             created_at = excluded.created_at,
             updated_at = excluded.updated_at",
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analytics_template_matches_the_orbit_contract() {
        let module = analytics_orbit_template();
        validate_orbit_module(&module).unwrap();
        assert_eq!(module.name, "Analytics");
        assert_eq!(module.section_key.as_deref(), Some("area"));
        assert_eq!(
            module
                .fields
                .iter()
                .map(|field| field.key.as_str())
                .collect::<Vec<_>>(),
            vec!["name", "what_it_does", "properties", "notes"]
        );
        assert!(module.agent_job.contains("Orbit is the analytics lexicon"));
        assert!(module
            .agent_job
            .contains("do not search for, create, or update"));
        assert!(module
            .agent_job
            .contains("Never access Choro's database directly"));
    }

    #[test]
    fn field_keys_are_stable_and_machine_readable() {
        assert_eq!(normalize_orbit_field_key(" What It Does "), "what_it_does");
        assert_eq!(normalize_orbit_field_key("Area / Screen"), "area_screen");
    }
}
