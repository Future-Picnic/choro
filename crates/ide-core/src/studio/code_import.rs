//! Read-only agent discovery followed by host-owned creation of selected drafts.
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StudioCodeSystem {
    pub name: String,
    pub platform: String,
    pub description: String,
    pub sources: BTreeMap<String, String>,
    pub system: StudioDesignSystem,
    #[serde(default)]
    pub assets: BTreeMap<String, String>,
    #[serde(default)]
    pub existing_system_id: Option<Uuid>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StudioCodeCandidate {
    pub id: Uuid,
    pub definition: StudioCodeSystem,
    pub source_hashes: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StudioCodeImport {
    pub id: Uuid,
    pub proposal_id: Uuid,
    pub scope_id: Option<Uuid>,
    pub summary: String,
    pub candidates: Vec<StudioCodeCandidate>,
    pub proposed: bool,
    #[serde(default)]
    pub created: BTreeSet<Uuid>,
}

impl StudioStore {
    fn code_import_path(&self, id: Uuid) -> PathBuf {
        self.cache.join("code-imports").join(format!("{id}.json"))
    }

    pub fn create_code_import(&self) -> Result<StudioCodeImport> {
        let import = StudioCodeImport {
            id: Uuid::new_v4(),
            proposal_id: Uuid::new_v4(),
            scope_id: None,
            summary: String::new(),
            candidates: vec![],
            proposed: false,
            created: BTreeSet::new(),
        };
        atomic(
            &self.code_import_path(import.id),
            &serde_json::to_vec(&import)?,
        )?;
        atomic(
            &self.cache.join("latest-code-import.json"),
            &serde_json::to_vec(&import.id)?,
        )?;
        Ok(import)
    }

    pub fn latest_code_import(&self) -> Result<Option<StudioCodeImport>> {
        match fs::read(self.cache.join("latest-code-import.json")) {
            Ok(bytes) => self.code_import(serde_json::from_slice(&bytes)?).map(Some),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub fn code_import(&self, id: Uuid) -> Result<StudioCodeImport> {
        let bytes = fs::read(self.code_import_path(id))?;
        ensure!(bytes.len() <= 2 * 1024 * 1024, "Import result is too large");
        let import: StudioCodeImport = serde_json::from_slice(&bytes)?;
        ensure!(import.id == id, "Import identity mismatch");
        Ok(import)
    }

    pub fn prepare_code_import(&self, agent: Uuid, context: &StudioAgentContext) -> Result<()> {
        ensure!(
            context.target == StudioAgentTarget::DesignSystemImport,
            "Not a code import"
        );
        let systems = self.systems()?;
        let _lock = self.lock()?;
        let mut import = self.code_import(context.design_id)?;
        let mut scope = StudioTurnScope::screen(import.id, Uuid::nil());
        scope.screen_ids.clear();
        scope.current_screen_id = None;
        import.scope_id = Some(scope.id);
        if import.created.is_empty() {
            import.proposed = false;
        }
        atomic(
            &self.code_import_path(import.id),
            &serde_json::to_vec(&import)?,
        )?;
        let request = serde_json::json!({
            "scope": scope, "import_id": import.id,
            "drafts_created": !import.created.is_empty(),
            "previous_analysis": import,
            "existing_systems": systems.iter().map(|system| serde_json::json!({"id":system.id,"name":system.name,"platform":system.platform,"sources":system.sources,"archived":system.archived})).collect::<Vec<_>>(),
            "context_rule": "Read project code and propose distinct design systems. Only the host can create the selected drafts. Existing systems and project source are read-only."
        });
        atomic(
            &self
                .cache
                .join("contexts")
                .join(format!("{}.json", scope.id)),
            &serde_json::to_vec(&request)?,
        )?;
        atomic(
            &self.cache.join("scopes").join(format!("{agent}.json")),
            &serde_json::to_vec(&scope)?,
        )?;
        atomic(
            &self.cache.join("roles").join(format!("{agent}.json")),
            &serde_json::to_vec(context)?,
        )
    }

    fn code_source_bytes(&self, relative: &str) -> Result<Vec<u8>> {
        ensure!(
            !relative.split('/').any(|part| matches!(
                part,
                ".git" | ".choro" | "node_modules" | "target" | "choro_designs"
            )),
            "Choose source files, not generated Studio data or dependencies"
        );
        let path = contained(&self.project, relative)?;
        ensure!(
            fs::metadata(&path)?.len() <= MAX_FILE as u64,
            "Source file exceeds 4 MiB"
        );
        Ok(fs::read(path)?)
    }

    /// Receipts tie claimed source evidence to the files this analysis read.
    pub fn record_code_import_read(&self, agent: Uuid, relative: &str, bytes: &[u8]) -> Result<()> {
        let _lock = self.lock()?;
        let scope = self.scope(agent)?;
        let import = self.code_import(scope.design_id)?;
        ensure!(
            import.scope_id == Some(scope.id),
            "Analysis changed while reading source"
        );
        let path = self
            .cache
            .join("code-import-reads")
            .join(format!("{}.json", scope.id));
        let mut reads: BTreeMap<String, String> = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(error) => return Err(error.into()),
        };
        reads.insert(relative.to_owned(), format!("{:x}", Sha256::digest(bytes)));
        ensure!(
            reads.len() <= 2048,
            "Narrow the analysis to the relevant apps"
        );
        atomic(&path, &serde_json::to_vec(&reads)?)
    }

    /// The model can save validated proposals only. IDs are always host-issued.
    pub fn propose_code_systems(
        &self,
        agent: Uuid,
        expected_scope: Uuid,
        summary: String,
        systems: Vec<StudioCodeSystem>,
    ) -> Result<StudioCodeImport> {
        let _lock = self.lock()?;
        let scope = self.scope(agent)?;
        ensure!(
            scope.id == expected_scope,
            "A newer analysis has started; read studio_context again"
        );
        let role: StudioAgentContext = serde_json::from_slice(&fs::read(
            self.cache.join("roles").join(format!("{agent}.json")),
        )?)?;
        ensure!(
            role.target == StudioAgentTarget::DesignSystemImport
                && role.design_id == scope.design_id,
            "This agent cannot propose code imports"
        );
        ensure!(
            !summary.trim().is_empty() && summary.len() <= 4000 && systems.len() <= 16,
            "Summarize up to 16 systems"
        );
        ensure!(
            serde_json::to_vec(&systems)?.len() <= 1024 * 1024,
            "System proposal exceeds 1 MiB"
        );
        let mut import = self.code_import(scope.design_id)?;
        ensure!(
            import.scope_id == Some(scope.id),
            "A newer analysis has started"
        );
        ensure!(
            import.created.is_empty(),
            "Start a new analysis after creating drafts"
        );
        let mut names = BTreeSet::new();
        let mut total = 0usize;
        let mut candidates = Vec::new();
        let receipts = self
            .cache
            .join("code-import-reads")
            .join(format!("{}.json", scope.id));
        let reads: BTreeMap<String, String> = match fs::read(receipts) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(error) => return Err(error.into()),
        };
        for mut definition in systems {
            validate_name(&definition.name)?;
            ensure!(
                names.insert(definition.name.to_lowercase()),
                "Give distinct systems distinct names"
            );
            ensure!(
                definition.platform.len() <= 100 && definition.description.len() <= 2000,
                "System description is too long"
            );
            ensure!(
                !definition.sources.is_empty() && definition.sources.len() <= 80,
                "Provide actual source evidence for each system"
            );
            definition.system.revision = 0;
            validate_system(&definition.system)?;
            let missing = missing_system_tokens(&definition.system);
            ensure!(missing.is_empty(), "Resolve these missing tokens before proposing a draft: {}. Recover their source values or use an explicit CSS fallback.", missing.into_iter().collect::<Vec<_>>().join(", "));
            ensure!(
                !definition.system.tokens.is_empty(),
                "No design tokens were recovered for this system"
            );
            if let Some(id) = definition.existing_system_id {
                self.load_inner(id)
                    .context("Existing system is unavailable")?;
                ensure!(self.is_system_workspace(id)?, "Expected a design system");
            }
            let mut source_hashes = BTreeMap::new();
            for (path, evidence) in &definition.sources {
                ensure!(
                    !evidence.trim().is_empty() && evidence.len() <= 2000,
                    "Describe the evidence in each source file"
                );
                let bytes = self.code_source_bytes(path)?;
                let hash = format!("{:x}", Sha256::digest(&bytes));
                ensure!(reads.get(path) == Some(&hash), "Read the current {path} with studio_project_read before citing it; source may have changed");
                total += bytes.len();
                ensure!(
                    total <= MAX_BUNDLE,
                    "Import sources exceed 64 MiB; narrow the analysis"
                );
                source_hashes.insert(path.clone(), hash);
            }
            ensure!(definition.assets.len() <= 40, "Too many imported fonts");
            for (name, path) in &definition.assets {
                ensure!(
                    definition
                        .system
                        .font_faces
                        .iter()
                        .any(|font| &font.file == name),
                    "Only declared local font assets can be imported"
                );
                let bytes = self.code_source_bytes(path)?;
                total += bytes.len();
                ensure!(total <= MAX_BUNDLE, "Import assets exceed the budget");
                source_hashes.insert(path.clone(), format!("{:x}", Sha256::digest(&bytes)));
            }
            ensure!(
                definition
                    .system
                    .font_faces
                    .iter()
                    .all(|font| definition.assets.contains_key(&font.file)),
                "Provide a source file for each bundled font"
            );
            candidates.push(StudioCodeCandidate {
                id: Uuid::new_v4(),
                definition,
                source_hashes,
            });
        }
        import.proposal_id = Uuid::new_v4();
        import.summary = summary;
        import.candidates = candidates;
        import.proposed = true;
        atomic(
            &self.code_import_path(import.id),
            &serde_json::to_vec(&import)?,
        )?;
        Ok(import)
    }

    pub fn verify_code_import_proposal(&self, agent: Uuid) -> Result<()> {
        let _lock = self.lock()?;
        let scope: StudioTurnScope = serde_json::from_slice(&fs::read(
            self.cache.join("scopes").join(format!("{agent}.json")),
        )?)?;
        let completion = self
            .cache
            .join("completed-turns")
            .join(format!("{}.json", scope.id));
        if !scope.active && completion.is_file() {
            return Ok(());
        }
        ensure!(
            scope.active && scope.expires_at > now(),
            "Analysis ended before completion"
        );
        let import = self.code_import(scope.design_id)?;
        ensure!(
            import.scope_id == Some(scope.id),
            "A newer analysis has started"
        );
        // Read-only analysis may end with a clarification question. Draft
        // creation stays unavailable until a validated proposal exists.
        atomic(
            &completion,
            &serde_json::to_vec(&(agent, scope.id, import.proposal_id))?,
        )?;
        Ok(())
    }

    /// Called only by the native Create drafts action, never by an agent tool.
    pub fn create_code_system_drafts(
        &self,
        import_id: Uuid,
        proposal_id: Uuid,
        selected: &BTreeSet<Uuid>,
    ) -> Result<Vec<Uuid>> {
        let _lock = self.lock()?;
        let mut import = self.code_import(import_id)?;
        ensure!(
            import.proposed && import.proposal_id == proposal_id,
            "Analysis changed; review the latest results"
        );
        ensure!(
            !selected.is_empty()
                && selected.iter().all(|id| import
                    .candidates
                    .iter()
                    .any(|candidate| &candidate.id == id)),
            "Select systems from this analysis"
        );
        // Validate every selected source before writing the first draft.
        for candidate in import.candidates.iter().filter(|candidate| {
            selected.contains(&candidate.id) && !import.created.contains(&candidate.id)
        }) {
            for (path, hash) in &candidate.source_hashes {
                ensure!(
                    &format!("{:x}", Sha256::digest(self.code_source_bytes(path)?)) == hash,
                    "Source changed: {path}. Analyze the project again before creating drafts"
                );
            }
        }
        let mut ids = Vec::new();
        for candidate in import
            .candidates
            .iter()
            .filter(|candidate| selected.contains(&candidate.id))
        {
            let definition = &candidate.definition;
            if let Some(existing) = definition.existing_system_id {
                ensure!(
                    self.is_system_workspace(existing)?,
                    "Existing system is unavailable"
                );
                ids.push(existing);
                continue;
            }
            let id = candidate.id;
            let path = self.path(&format!("{DESIGNS_DIR}/design-systems/{id}/system.json"))?;
            // A retry uses the same host-issued ID and never overwrites its draft.
            if !path.exists() {
                for (name, source) in &definition.assets {
                    let bytes = self.code_source_bytes(source)?;
                    ensure!(
                        candidate.source_hashes.get(source)
                            == Some(&format!("{:x}", Sha256::digest(&bytes))),
                        "Font source changed; analyze the project again"
                    );
                    atomic(
                        &self.path(&format!("{DESIGNS_DIR}/design-systems/{id}/assets/{name}"))?,
                        &bytes,
                    )?;
                }
                let record = StudioSystemRecord {
                    schema_version: SCHEMA_VERSION,
                    id,
                    name: definition.name.clone(),
                    platform: definition.platform.clone(),
                    archived: false,
                    sources: definition.sources.clone(),
                    draft: definition.system.clone(),
                    applied: None,
                    applied_assets: Some(BTreeSet::new()),
                };
                atomic(&path, &serde_json::to_vec_pretty(&record)?)?;
            }
            import.created.insert(id);
            ids.push(id);
        }
        atomic(
            &self.code_import_path(import_id),
            &serde_json::to_vec(&import)?,
        )?;
        Ok(ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, StudioStore, StudioAgentContext, Uuid) {
        let temp = tempfile::tempdir().unwrap();
        let store = StudioStore::new(temp.path(), temp.path().join("cache")).unwrap();
        fs::create_dir_all(temp.path().join("apps/web")).unwrap();
        fs::create_dir_all(temp.path().join("apps/admin")).unwrap();
        fs::write(
            temp.path().join("apps/web/theme.css"),
            ":root { --color-primary: #123456; }",
        )
        .unwrap();
        fs::write(
            temp.path().join("apps/admin/theme.css"),
            ":root { --color-primary: #abcdef; }",
        )
        .unwrap();
        let import = store.create_code_import().unwrap();
        let context = StudioAgentContext {
            target: StudioAgentTarget::DesignSystemImport,
            design_id: import.id,
            conversation_id: import.id,
        };
        let agent = Uuid::new_v4();
        store.prepare_code_import(agent, &context).unwrap();
        for relative in ["apps/web/theme.css", "apps/admin/theme.css"] {
            store
                .record_code_import_read(
                    agent,
                    relative,
                    &fs::read(temp.path().join(relative)).unwrap(),
                )
                .unwrap();
        }
        (temp, store, context, agent)
    }

    fn candidate(name: &str, file: &str, color: &str) -> StudioCodeSystem {
        StudioCodeSystem {
            name: name.into(),
            platform: "Web".into(),
            description: format!("{name} has its own theme."),
            sources: [(file.into(), format!("Defines color-primary as {color}."))].into(),
            system: StudioDesignSystem {
                tokens: [("color-primary".into(), color.into())].into(),
                ..empty_system()
            },
            assets: BTreeMap::new(),
            existing_system_id: None,
        }
    }

    fn propose(
        store: &StudioStore,
        agent: Uuid,
        candidates: Vec<StudioCodeSystem>,
    ) -> StudioCodeImport {
        store
            .propose_code_systems(
                agent,
                store.scope(agent).unwrap().id,
                "Found independent web and admin themes.".into(),
                candidates,
            )
            .unwrap()
    }

    #[test]
    fn follow_up_after_creation_keeps_findings_and_drafts_available() {
        let (_temp, store, context, agent) = fixture();
        let proposal = propose(
            &store,
            agent,
            vec![candidate("Web", "apps/web/theme.css", "#123456")],
        );
        let selected = BTreeSet::from([proposal.candidates[0].id]);
        let ids = store
            .create_code_system_drafts(context.design_id, proposal.proposal_id, &selected)
            .unwrap();
        let draft = store.load(ids[0]).unwrap();
        store.prepare_code_import(agent, &context).unwrap();
        let follow_up = store.code_import(context.design_id).unwrap();
        assert!(follow_up.proposed);
        assert_eq!(follow_up.proposal_id, proposal.proposal_id);
        assert_eq!(follow_up.created, selected);
        let request = store.request_context(agent).unwrap();
        assert_eq!(request["drafts_created"], true);
        assert_eq!(
            request["previous_analysis"]["candidates"][0]["definition"]["system"]["tokens"]
                ["color-primary"],
            "#123456"
        );
        assert!(store
            .propose_code_systems(
                agent,
                store.scope(agent).unwrap().id,
                "Replacement".into(),
                vec![]
            )
            .is_err());
        store.verify_code_import_proposal(agent).unwrap();
        assert_eq!(store.load(ids[0]).unwrap(), draft);
    }

    #[test]
    fn code_import_rejects_unresolved_recipe_references_before_creating_drafts() {
        let (_temp, store, context, agent) = fixture();
        let mut definition = candidate("Web", "apps/web/theme.css", "#123456");
        definition.system.recipes.insert(
            "button".into(),
            [("background".into(), "var(--new-btn-primary)".into())].into(),
        );
        let error = store
            .propose_code_systems(
                agent,
                store.scope(agent).unwrap().id,
                "Web theme".into(),
                vec![definition.clone()],
            )
            .unwrap_err();
        assert!(error.to_string().contains("new-btn-primary"));
        assert!(!store.code_import(context.design_id).unwrap().proposed);
        assert!(store.systems().unwrap().is_empty());
        definition
            .system
            .tokens
            .insert("new-btn-primary".into(), "var(--color-primary)".into());
        assert!(propose(&store, agent, vec![definition]).proposed);
    }

    #[test]
    fn code_import_creates_distinct_drafts_only_after_host_selection_and_survives_reopen() {
        let (temp, store, context, agent) = fixture();
        let proposal = propose(
            &store,
            agent,
            vec![
                candidate("Web", "apps/web/theme.css", "#123456"),
                candidate("Admin", "apps/admin/theme.css", "#abcdef"),
            ],
        );
        assert!(store.systems().unwrap().is_empty());
        assert!(!temp
            .path()
            .join(DESIGNS_DIR)
            .join("design-systems")
            .join("default.json")
            .exists());
        let selected = proposal
            .candidates
            .iter()
            .map(|candidate| candidate.id)
            .collect();
        let ids = store
            .create_code_system_drafts(context.design_id, proposal.proposal_id, &selected)
            .unwrap();
        assert_eq!(ids.len(), 2);
        assert_eq!(
            store.system(ids[0]).unwrap().draft.tokens["color-primary"],
            "#123456"
        );
        assert_eq!(
            store.system(ids[1]).unwrap().draft.tokens["color-primary"],
            "#abcdef"
        );
        assert!(store
            .systems()
            .unwrap()
            .iter()
            .all(|system| system.applied.is_none()));
        assert_eq!(store.default_system_id().unwrap(), None);
        assert_eq!(
            fs::read_to_string(temp.path().join("apps/web/theme.css")).unwrap(),
            ":root { --color-primary: #123456; }"
        );
        let reopened = StudioStore::new(temp.path(), temp.path().join("cache")).unwrap();
        assert_eq!(
            reopened
                .latest_code_import()
                .unwrap()
                .unwrap()
                .created
                .len(),
            2
        );
        assert_eq!(
            reopened
                .create_code_system_drafts(context.design_id, proposal.proposal_id, &selected)
                .unwrap(),
            ids
        );
        assert_eq!(reopened.systems().unwrap().len(), 2);
    }

    #[test]
    fn code_import_rejects_changed_sources_before_creating_any_selected_draft() {
        let (temp, store, context, agent) = fixture();
        let proposal = propose(
            &store,
            agent,
            vec![
                candidate("Web", "apps/web/theme.css", "#123456"),
                candidate("Admin", "apps/admin/theme.css", "#abcdef"),
            ],
        );
        let selected = proposal
            .candidates
            .iter()
            .map(|candidate| candidate.id)
            .collect();
        assert!(store
            .create_code_system_drafts(context.design_id, Uuid::new_v4(), &selected)
            .is_err());
        fs::write(
            temp.path().join("apps/admin/theme.css"),
            ":root { --color-primary: red; }",
        )
        .unwrap();
        assert!(store
            .create_code_system_drafts(context.design_id, proposal.proposal_id, &selected)
            .unwrap_err()
            .to_string()
            .contains("Source changed"));
        assert!(store.systems().unwrap().is_empty());
    }

    #[test]
    fn code_import_evidence_must_match_the_source_actually_read() {
        let (temp, store, context, agent) = fixture();
        fs::write(
            temp.path().join("apps/web/theme.css"),
            ":root { --color-primary: #654321; }",
        )
        .unwrap();
        let definition = candidate("Web", "apps/web/theme.css", "#123456");
        assert!(store
            .propose_code_systems(
                agent,
                store.scope(agent).unwrap().id,
                "Web theme".into(),
                vec![definition]
            )
            .unwrap_err()
            .to_string()
            .contains("Read the current"));
        assert!(!store.code_import(context.design_id).unwrap().proposed);
        store
            .record_code_import_read(
                agent,
                "apps/web/theme.css",
                &fs::read(temp.path().join("apps/web/theme.css")).unwrap(),
            )
            .unwrap();
        let result = propose(
            &store,
            agent,
            vec![candidate("Web", "apps/web/theme.css", "#654321")],
        );
        assert!(result.proposed);
        assert!(store.systems().unwrap().is_empty());
    }

    #[test]
    fn code_import_is_scoped_and_stale_or_stopped_agents_cannot_propose() {
        let (_temp, store, context, agent) = fixture();
        let scope = store.scope(agent).unwrap();
        store.prepare_code_import(agent, &context).unwrap();
        assert!(store
            .propose_code_systems(agent, scope.id, "No UI".into(), vec![])
            .is_err());
        let mut current = store.scope(agent).unwrap();
        current.active = false;
        store.save_scope(agent, &current).unwrap();
        assert!(store
            .propose_code_systems(agent, current.id, "No UI".into(), vec![])
            .is_err());
        assert!(store.verify_code_import_proposal(agent).is_err());
        store.prepare_code_import(agent, &context).unwrap();
        let mut ordinary_role = context.clone();
        ordinary_role.target = StudioAgentTarget::DesignSystem;
        atomic(
            &store.cache.join("roles").join(format!("{agent}.json")),
            &serde_json::to_vec(&ordinary_role).unwrap(),
        )
        .unwrap();
        assert!(store
            .propose_code_systems(
                agent,
                store.scope(agent).unwrap().id,
                "No UI".into(),
                vec![]
            )
            .is_err());
    }

    #[test]
    fn code_import_existing_systems_are_never_overwritten_and_only_selected_new_systems_are_created(
    ) {
        let (_temp, store, context, agent) = fixture();
        let existing = store.create_system("Shared UI", "Web", None).unwrap();
        let mut found = candidate("Shared UI", "apps/web/theme.css", "#123456");
        found.existing_system_id = Some(existing.id);
        let proposal = propose(
            &store,
            agent,
            vec![found, candidate("Admin", "apps/admin/theme.css", "#abcdef")],
        );
        let selected = [proposal.candidates[0].id].into();
        assert_eq!(
            store
                .create_code_system_drafts(context.design_id, proposal.proposal_id, &selected)
                .unwrap(),
            vec![existing.id]
        );
        assert_eq!(store.system(existing.id).unwrap(), existing);
        assert_eq!(store.systems().unwrap().len(), 1);
        let selected = [proposal.candidates[1].id].into();
        store
            .create_code_system_drafts(context.design_id, proposal.proposal_id, &selected)
            .unwrap();
        assert_eq!(store.systems().unwrap().len(), 2);
    }

    #[test]
    fn code_import_handles_no_ui_and_clarification_without_phantom_drafts() {
        let (_temp, store, context, agent) = fixture();
        // Read-only questions can end a turn without a proposal.
        store.verify_code_import_proposal(agent).unwrap();
        assert!(!store.code_import(context.design_id).unwrap().proposed);
        let proposal = propose(&store, agent, vec![]);
        assert!(proposal.proposed && proposal.candidates.is_empty());
        store.verify_code_import_proposal(agent).unwrap();
        let mut scope = store.scope(agent).unwrap();
        scope.active = false;
        store.save_scope(agent, &scope).unwrap();
        store.verify_code_import_proposal(agent).unwrap();
        assert!(store.systems().unwrap().is_empty());
    }

    #[test]
    fn code_import_rejects_uncontained_sources_and_copies_declared_local_fonts() {
        let (temp, store, context, agent) = fixture();
        assert!(store
            .propose_code_systems(
                agent,
                store.scope(agent).unwrap().id,
                "Found UI".into(),
                vec![candidate("Bad", "../theme.css", "#123456")]
            )
            .is_err());
        let mut found = candidate("Web", "apps/web/theme.css", "#123456");
        fs::write(
            temp.path().join("apps/web/font.woff2"),
            b"wOF2-local-font-test",
        )
        .unwrap();
        found.system.font_faces.push(StudioFontFace {
            family: "Local UI".into(),
            file: "ui.woff2".into(),
            weight: 400,
            italic: false,
        });
        found
            .assets
            .insert("ui.woff2".into(), "apps/web/font.woff2".into());
        let proposal = propose(&store, agent, vec![found]);
        let ids = store
            .create_code_system_drafts(
                context.design_id,
                proposal.proposal_id,
                &[proposal.candidates[0].id].into(),
            )
            .unwrap();
        assert_eq!(
            store.assets(ids[0]).unwrap()["design-system/assets/ui.woff2"],
            b"wOF2-local-font-test"
        );
    }
}
