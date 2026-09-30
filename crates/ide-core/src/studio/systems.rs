//! Named systems use the same revisioned transaction and agent machinery as screens.
//! Their workspace is a generated specimen document; writes edit the draft, never consumers.
use super::*;

pub const LEGACY_SYSTEM_ID: Uuid = Uuid::from_u128(0x63686f726f0000000000000000000001);
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct StudioSystemRecord {
    pub schema_version: u32,
    pub id: Uuid,
    pub name: String,
    pub platform: String,
    pub archived: bool,
    pub sources: BTreeMap<String, String>,
    pub draft: StudioDesignSystem,
    pub applied: Option<StudioDesignSystem>,
    #[serde(default)]
    pub applied_assets: Option<BTreeSet<String>>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct StudioSystemImpact {
    pub id: Uuid,
    pub name: String,
    pub fingerprint: String,
}
pub fn system_reference(id: Uuid) -> String {
    format!("../design-systems/{id}/system.json")
}
pub fn system_id_from_reference(reference: &str) -> Result<Option<Uuid>> {
    if reference.is_empty() {
        return Ok(None);
    }
    if reference == "../design-system/system.json" {
        return Ok(Some(LEGACY_SYSTEM_ID));
    }
    let id = reference
        .strip_prefix("../design-systems/")
        .and_then(|s| s.strip_suffix("/system.json"))
        .context("Unknown design-system reference; choose a system from the library")?;
    Ok(Some(id.parse()?))
}
pub fn empty_system() -> StudioDesignSystem {
    StudioDesignSystem {
        schema_version: SCHEMA_VERSION,
        revision: 0,
        tokens: BTreeMap::new(),
        recipes: BTreeMap::new(),
        font_faces: Vec::new(),
    }
}
fn system_file(id: Uuid) -> String {
    format!("{DESIGNS_DIR}/design-systems/{id}/system.json")
}
fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
pub(super) fn validate_system_binding(design: &StudioDesign) -> Result<()> {
    // Explicit CSS fallbacks are safe. Unresolved references must be mapped before switching.
    let tokens = design.tokens();
    let mut missing = BTreeSet::new();
    let declaration =
        regex::Regex::new(r"--([a-zA-Z0-9-]+)\s*:").expect("static CSS declaration expression");
    let local_names = design
        .documents
        .values()
        .flat_map(|doc| {
            declaration
                .captures_iter(&doc.css)
                .chain(declaration.captures_iter(&doc.html))
                .map(|c| c[1].to_string())
        })
        .collect::<BTreeSet<_>>();
    let mut inspect = |text: &str| {
        for tail in text.split("var(--").skip(1) {
            let Some(end) = tail.find(')') else { continue };
            let value = &tail[..end];
            if !value.contains(',')
                && !tokens.contains_key(value.trim())
                && !local_names.contains(value.trim())
            {
                missing.insert(value.trim().to_string());
            }
        }
    };
    for value in tokens.values() {
        inspect(value);
    }
    for doc in design.documents.values() {
        inspect(&doc.css);
        inspect(&doc.html);
    }
    for recipe in design.system.recipes.values() {
        for value in recipe.values() {
            inspect(value);
        }
    }
    ensure!(
        missing.is_empty(),
        "Map these missing tokens before changing systems: {}",
        missing.into_iter().collect::<Vec<_>>().join(", ")
    );
    Ok(())
}
impl StudioStore {
    pub fn is_system_workspace(&self, id: Uuid) -> Result<bool> {
        Ok(self.path(&system_file(id))?.exists())
    }
    pub(super) fn system_inner(&self, id: Uuid) -> Result<StudioSystemRecord> {
        let record: StudioSystemRecord =
            serde_json::from_slice(&self.read_file(&system_file(id))?)?;
        ensure!(
            record.id == id && record.schema_version == SCHEMA_VERSION,
            "Unsupported design system"
        );
        validate_name(&record.name)?;
        validate_system(&record.draft)?;
        if let Some(system) = &record.applied {
            validate_system(system)?;
        }
        Ok(record)
    }
    pub fn design_system_context(&self, design: &StudioDesign) -> Result<serde_json::Value> {
        let Some(id) = system_id_from_reference(&design.manifest.design_system)? else {
            return Ok(
                serde_json::json!({"selected":false,"guidance":"No system selected. Use local overrides or ask the user to choose or create a system."}),
            );
        };
        if !self.is_system_workspace(id)? {
            return Ok(
                serde_json::json!({"id":id,"name":"Legacy project system","revision":design.system.revision}),
            );
        }
        let record = self.system_inner(id)?;
        Ok(
            serde_json::json!({"id":id,"name":record.name,"platform":record.platform,"sources":record.sources,"revision":design.system.revision,"draft_workspace":design.manifest.system_workspace,"font_faces":design.system.font_faces}),
        )
    }
    pub fn system(&self, id: Uuid) -> Result<StudioSystemRecord> {
        let _lock = self.lock()?;
        self.recover()?;
        self.system_inner(id)
    }
    pub(super) fn migrate_legacy_system(&self) -> Result<()> {
        let legacy = self.path(&format!("{DESIGNS_DIR}/design-system/system.json"))?;
        if legacy.exists() && !self.is_system_workspace(LEGACY_SYSTEM_ID)? {
            let system: StudioDesignSystem = serde_json::from_slice(&fs::read(legacy)?)?;
            validate_system(&system)?;
            let record = StudioSystemRecord {
                schema_version: SCHEMA_VERSION,
                id: LEGACY_SYSTEM_ID,
                name: if system == StudioDesignSystem::default() {
                    "Legacy starter"
                } else {
                    "Imported project system"
                }
                .into(),
                platform: "Unspecified".into(),
                archived: false,
                sources: BTreeMap::from([(
                    "choro_designs/design-system/system.json".into(),
                    "Imported without changing existing designs".into(),
                )]),
                applied: Some(system.clone()),
                draft: system,
                applied_assets: None,
            };
            atomic(
                &self.path(&system_file(record.id))?,
                &serde_json::to_vec_pretty(&record)?,
            )?;
        }
        Ok(())
    }
    pub fn systems(&self) -> Result<Vec<StudioSystemRecord>> {
        let _lock = self.lock()?;
        self.recover()?;
        self.migrate_legacy_system()?;
        let directory = self.path(&format!("{DESIGNS_DIR}/design-systems"))?;
        if !directory.exists() {
            return Ok(vec![]);
        }
        let mut records = vec![];
        for entry in fs::read_dir(directory)? {
            if let Ok(id) = entry?.file_name().to_string_lossy().parse::<Uuid>() {
                records.push(self.system_inner(id)?);
            }
        }
        records.sort_by_key(|s| (s.archived, s.name.to_lowercase()));
        Ok(records)
    }
    pub fn create_system(
        &self,
        name: &str,
        platform: &str,
        duplicate: Option<Uuid>,
    ) -> Result<StudioSystemRecord> {
        let _lock = self.lock()?;
        self.recover()?;
        validate_name(name)?;
        ensure!(platform.len() <= 100, "Platform description is too long");
        let mut record = if let Some(id) = duplicate {
            self.system_inner(id)?
        } else {
            StudioSystemRecord {
                schema_version: SCHEMA_VERSION,
                id: Uuid::new_v4(),
                name: String::new(),
                platform: platform.into(),
                archived: false,
                sources: BTreeMap::new(),
                draft: empty_system(),
                applied: None,
                applied_assets: Some(BTreeSet::new()),
            }
        };
        record.id = Uuid::new_v4();
        record.name = name.trim().into();
        record.archived = false;
        record.platform = platform.into();
        record.draft.revision = 0;
        record.applied = None;
        record.applied_assets = Some(BTreeSet::new());
        if let Some(source) = duplicate {
            for (name, bytes) in self.assets(source)? {
                if let Some(name) = name.strip_prefix("design-system/assets/") {
                    atomic(
                        &self.path(&format!(
                            "{DESIGNS_DIR}/design-systems/{}/assets/{name}",
                            record.id
                        ))?,
                        &bytes,
                    )?;
                }
            }
        }
        atomic(
            &self.path(&system_file(record.id))?,
            &serde_json::to_vec_pretty(&record)?,
        )?;
        Ok(record)
    }
    pub(super) fn resolve_system(&self, reference: &str) -> Result<StudioDesignSystem> {
        let Some(id) = system_id_from_reference(reference)? else {
            return Ok(empty_system());
        };
        if id == LEGACY_SYSTEM_ID && !self.is_system_workspace(id)? {
            return Ok(serde_json::from_slice(&self.read_file(&format!(
                "{DESIGNS_DIR}/design-system/system.json"
            ))?)?);
        }
        self.system_inner(id)?.applied.context(
            "This design system has no applied version. Open it and review its draft first.",
        )
    }
    pub fn set_default_system(&self, id: Option<Uuid>) -> Result<()> {
        let _lock = self.lock()?;
        if let Some(id) = id {
            let r = self.system_inner(id)?;
            ensure!(
                !r.archived && r.applied.is_some(),
                "Choose an applied, active system"
            );
        }
        atomic(
            &self.path(&format!("{DESIGNS_DIR}/design-systems/default.json"))?,
            &serde_json::to_vec(&id)?,
        )
    }
    pub fn default_system_id(&self) -> Result<Option<Uuid>> {
        let _lock = self.lock()?;
        system_id_from_reference(&self.default_system_reference()?)
    }
    pub(super) fn default_system_reference(&self) -> Result<String> {
        let path = self.path(&format!("{DESIGNS_DIR}/design-systems/default.json"))?;
        if !path.exists() {
            return Ok(String::new());
        }
        let id: Option<Uuid> = serde_json::from_slice(&fs::read(path)?)?;
        Ok(match id {
            Some(id)
                if self
                    .system_inner(id)
                    .is_ok_and(|s| !s.archived && s.applied.is_some()) =>
            {
                system_reference(id)
            }
            _ => String::new(),
        })
    }
    pub fn archive_system(&self, id: Uuid, expected_revision: u64, archived: bool) -> Result<()> {
        let _lock = self.lock()?;
        self.recover()?;
        let mut record = self.system_inner(id)?;
        ensure!(
            record.draft.revision == expected_revision,
            "System changed; refresh before archiving"
        );
        record.archived = archived;
        atomic(
            &self.path(&system_file(id))?,
            &serde_json::to_vec_pretty(&record)?,
        )
    }
    pub(super) fn system_impact_inner(&self, id: Uuid) -> Result<Vec<StudioSystemImpact>> {
        let root = self.path(DESIGNS_DIR)?;
        let mut result = vec![];
        if root.exists() {
            for entry in fs::read_dir(root)? {
                let Ok(design_id) = entry?.file_name().to_string_lossy().parse::<Uuid>() else {
                    continue;
                };
                let design = self.load_inner(design_id)?;
                if system_id_from_reference(&design.manifest.design_system)? == Some(id) {
                    result.push(StudioSystemImpact {
                        id: design_id,
                        name: design.manifest.name,
                        fingerprint: design.fingerprint,
                    });
                }
            }
        }
        result.sort_by_key(|d| d.id);
        Ok(result)
    }
    pub fn system_impact(&self, id: Uuid) -> Result<Vec<StudioSystemImpact>> {
        let _lock = self.lock()?;
        self.recover()?;
        self.system_impact_inner(id)
    }
    pub fn system_binding_fingerprint(&self, id: Uuid) -> Result<String> {
        let record = self.system_inner(id)?;
        let assets = self
            .assets(id)?
            .into_iter()
            .filter(|(name, _)| {
                name.starts_with("design-system/assets/")
                    && record
                        .applied_assets
                        .as_ref()
                        .is_none_or(|allowed| allowed.contains(name))
            })
            .collect::<BTreeMap<_, _>>();
        Ok(hash(&serde_json::to_vec(&(record.applied, assets))?))
    }
    pub fn publish_system(
        &self,
        id: Uuid,
        expected_revision: u64,
        expected_fingerprint: &str,
        impacts: &[StudioSystemImpact],
    ) -> Result<()> {
        let _lock = self.lock()?;
        self.recover()?;
        let mut record = self.system_inner(id)?;
        ensure!(
            !record.archived,
            "Restore this system before applying changes"
        );
        ensure!(
            record.draft.revision == expected_revision
                && self.load_inner(id)?.fingerprint == expected_fingerprint
                && self.system_impact_inner(id)? == impacts,
            "System or linked designs changed; review changes again"
        );
        ensure!(
            !record.draft.tokens.is_empty(),
            "Build the system before applying it"
        );
        for impact in impacts {
            let mut design = self.load_inner(impact.id)?;
            self.remember_revision(&design)?;
            design.system = record.draft.clone();
            validate_system_binding(&design)?;
        }
        validate_system_binding(&self.load_inner(id)?)?;
        let assets = self.assets(id)?;
        ensure!(
            record
                .draft
                .font_faces
                .iter()
                .all(|font| assets.contains_key(&format!("design-system/assets/{}", font.file))),
            "Bundle the referenced font files before applying this system"
        );
        // One atomic publication: all readers resolve the same applied version.
        atomic(
            &self
                .cache
                .join("system-publications")
                .join(format!("{id}-{}.json", record.draft.revision)),
            &serde_json::to_vec(&record)?,
        )?;
        record.applied = Some(record.draft.clone());
        record.applied_assets = Some(
            self.assets(id)?
                .keys()
                .filter(|name| name.starts_with("design-system/assets/"))
                .cloned()
                .collect(),
        );
        atomic(
            &self.path(&system_file(id))?,
            &serde_json::to_vec_pretty(&record)?,
        )
    }
    /// Save isolated comparison bundles in app data. No project file or live editor is changed.
    pub fn system_comparison(
        &self,
        workspace: Uuid,
        target: Option<Uuid>,
        publish: bool,
    ) -> Result<Vec<(String, StudioDesign, StudioDesign)>> {
        let _lock = self.lock()?;
        self.recover()?;
        let design = self.load_inner(workspace)?;
        let mut examples = vec![design];
        if publish {
            if let Some(consumer) = self.system_impact_inner(workspace)?.first() {
                examples.push(self.load_inner(consumer.id)?);
            }
        }
        let mut result = vec![];
        for example in examples {
            let mut before = example.clone();
            let mut after = example.clone();
            let before_assets = self.assets(example.manifest.id)?;
            let mut after_assets = before_assets.clone();
            let record = target.map(|id| self.system_inner(id)).transpose()?;
            after.system = if publish {
                record.as_ref().context("Missing system")?.draft.clone()
            } else {
                record
                    .as_ref()
                    .and_then(|r| r.applied.clone())
                    .unwrap_or_else(empty_system)
            };
            if example.manifest.system_workspace {
                let record = record.as_ref().context("Missing system")?;
                let mut applied = record.clone();
                applied.draft = record.applied.clone().unwrap_or_else(empty_system);
                before.system = applied.draft.clone();
                before
                    .documents
                    .insert(workspace, system_specimen(&applied));
            }
            after_assets.retain(|name, _| !name.starts_with("design-system/assets/"));
            if let Some(record) = record.as_ref() {
                for (name, bytes) in self.assets(record.id)? {
                    if name.starts_with("design-system/assets/")
                        && (publish
                            || record
                                .applied_assets
                                .as_ref()
                                .is_none_or(|allowed| allowed.contains(&name)))
                    {
                        after_assets.insert(name, bytes);
                    }
                }
            }
            result.push((
                example.manifest.name.clone(),
                self.cache_comparison(before, before_assets)?,
                self.cache_comparison(after, after_assets)?,
            ));
        }
        Ok(result)
    }
    fn cache_comparison(
        &self,
        mut design: StudioDesign,
        assets: BTreeMap<String, Vec<u8>>,
    ) -> Result<StudioDesign> {
        let screen = design
            .manifest
            .screens
            .iter()
            .find(|s| !s.archived)
            .cloned();
        let document = screen.as_ref().and_then(|screen|design.documents.get(&screen.id)).cloned().unwrap_or_else(||StudioDocument{html:"<!doctype html><html><body><h1>No active screens</h1><p>The selected system will be used when you add a screen.</p></body></html>".into(),css:"body{font:20px system-ui;padding:48px;background:white;color:#202124}".into(),js:String::new()});
        let screen = screen.unwrap_or(StudioScreen {
            id: Uuid::new_v4(),
            name: "No active screens".into(),
            width: 1120,
            height: 800,
            archived: false,
            files: StudioScreenFiles::default(),
        });
        let cache_key = hash(&serde_json::to_vec(&(
            design.manifest.id,
            &document,
            screen.width,
            screen.height,
            &design.system,
            &design.overrides,
            &assets,
        ))?);
        let id = Uuid::from_u128(u128::from_str_radix(&cache_key[..32], 16)?);
        let mut screen = screen;
        screen.id = id;
        design.manifest.id = id;
        design.manifest.screens = vec![screen];
        design.documents = BTreeMap::from([(id, document)]);
        design.asset_fingerprint = hash(&serde_json::to_vec(&assets)?);
        design.fingerprint.clear();
        design.fingerprint = hash(&serde_json::to_vec(&design)?);
        let path = self
            .cache
            .join("revisions")
            .join(id.to_string())
            .join(format!(
                "{}-{}.json",
                design.manifest.revision, design.fingerprint
            ));
        atomic(
            &path,
            &serde_json::to_vec(&StudioSavedRevision {
                design: design.clone(),
                assets,
            })?,
        )?;
        Ok(design)
    }
    pub(super) fn load_system_workspace(&self, id: Uuid) -> Result<StudioDesign> {
        let record = self.system_inner(id)?;
        let screen = StudioScreen {
            id,
            name: "System specimen".into(),
            width: 1120,
            height: (1500
                + record.draft.tokens.len() as u32 * 26
                + record.sources.len() as u32 * 100)
                .min(4096),
            archived: false,
            files: StudioScreenFiles::default(),
        };
        let manifest = StudioDesignManifest {
            schema_version: SCHEMA_VERSION,
            id,
            name: record.name.clone(),
            revision: record.draft.revision,
            design_system: system_reference(id),
            system_workspace: true,
            screens: vec![screen],
            source_doc: None,
            source_task: None,
            source_context: BTreeMap::from([(
                "system".into(),
                StudioSource {
                    task_ref: None,
                    reference: record.platform.clone(),
                    content: serde_json::to_string(&record.sources)?,
                },
            )]),
        };
        let mut design = StudioDesign {
            manifest,
            documents: BTreeMap::from([(id, system_specimen(&record))]),
            system: record.draft,
            overrides: StudioOverrides::default(),
            fingerprint: String::new(),
            asset_fingerprint: hash(&serde_json::to_vec(&self.assets(id)?)?),
        };
        design.fingerprint = hash(&serde_json::to_vec(&design)?);
        Ok(design)
    }
    pub(super) fn apply_system_edit(
        &self,
        scope: &StudioTurnScope,
        tx: &StudioTransaction,
        before: StudioDesign,
        history: Option<HistoryAction>,
    ) -> Result<StudioDesign> {
        ensure!(
            scope.allow_shared_system,
            "This turn cannot edit a design system"
        );
        let mut record = self.system_inner(tx.design_id)?;
        ensure!(!record.archived, "Restore this system before editing");
        let mut writes = BTreeMap::new();
        for operation in &tx.operations {
            match operation {
                StudioOperation::SetSystem { system, expected_system_revision } => {
                    ensure!(*expected_system_revision == before.system.revision, "System draft changed; read it again"); validate_system(system)?; record.draft = system.clone();
                }
                StudioOperation::RenameDesign { name } => { validate_name(name)?; record.name = name.trim().into(); }
                StudioOperation::SystemDetails { platform, sources } => {
                    ensure!(platform.len() <= 100 && sources.len() <= 200 && sources.iter().all(|(k,v)| k.len() <= 1024 && v.len() <= 8192), "System source metadata is too large");
                    record.platform = platform.clone(); record.sources = sources.clone();
                }
                StudioOperation::AddAsset { name, bytes } => {
                    ensure!(name.len() <= 160 && Path::new(name).components().count() == 1 && !name.starts_with('.'), "Use a plain asset filename");
                    ensure!(!bytes.is_empty() && bytes.len() <= MAX_FILE, "Asset exceeds the file limit");
                    let extension = Path::new(name).extension().and_then(|s|s.to_str()).unwrap_or("");
                    ensure!(["png","jpg","jpeg","gif","webp","svg","woff","woff2","ttf","otf"].contains(&extension), "Use an image or font asset");
                    let path = format!("{DESIGNS_DIR}/design-systems/{}/assets/{name}", record.id);
                    ensure!(!self.path(&path)?.exists(), "Asset exists; use a new filename"); writes.insert(path, bytes.clone());
                }
                _ => bail!("System agents may edit only system draft tokens, recipes, metadata and assets. They cannot edit screens or apply shared changes."),
            }
        }
        record.draft.revision = before.manifest.revision + 1;
        writes.insert(system_file(record.id), serde_json::to_vec_pretty(&record)?);
        ensure!(
            writes.values().all(|v| v.len() <= MAX_FILE),
            "System exceeds the file limit"
        );
        let assets = self.assets(record.id)?;
        ensure!(
            assets.len() + writes.len() <= 1001
                && assets.values().map(Vec::len).sum::<usize>()
                    + writes.values().map(Vec::len).sum::<usize>()
                    <= MAX_BUNDLE,
            "System assets exceed the bundle limit"
        );
        let mut previous = BTreeMap::new();
        for path in writes.keys() {
            let pathbuf = self.path(path)?;
            previous.insert(
                path.clone(),
                if pathbuf.exists() {
                    Some(fs::read(pathbuf)?)
                } else {
                    None
                },
            );
        }
        self.remember_revision(&before)?;
        let mut journal = Journal {
            request: tx.clone(),
            after: before.clone(),
            before,
            writes,
            previous,
            committed: false,
            history,
        };
        let path = self
            .cache
            .join("transactions")
            .join(format!("{}.json", tx.id));
        atomic(&path, &serde_json::to_vec(&journal)?)?;
        self.finish_journal(&mut journal, &path)?;
        Ok(journal.after)
    }
}
/// Deterministic, script-free specimens rendered by Studio's existing isolated renderer.
pub fn system_specimen(record: &StudioSystemRecord) -> StudioDocument {
    let mut html = format!("<!doctype html><html><head><meta charset=\"utf-8\"></head><body><main><header><h1>{}</h1><p>{}</p></header>", escape(&record.name), escape(&record.platform));
    if record.draft.tokens.is_empty() {
        html.push_str("<section><h2>Build your design system</h2><p>Use the agent to extract styles from your app, or describe the system you want to create. Your colors, typography and components will appear here.</p></section>");
    } else {
        html.push_str("<section id=\"system-typography\"><h2>Typography</h2><div class=\"type-sample\"><h1 class=\"ds-heading\" role=\"button\" tabindex=\"0\" data-system-token=\"font-size-heading\">A clear voice for your product</h1><p>Choose the words, size and rhythm that make your interface feel familiar.</p><small>Labels, captions and supporting information</small></div></section><section id=\"system-colors\"><h2>Colors</h2><div class=\"swatches\">");
        for (name, value) in &record.draft.tokens {
            if name.contains("color") {
                html.push_str(&format!("<div role=\"button\" tabindex=\"0\" data-system-token=\"{}\"><div class=\"swatch\" style=\"background:var(--{})\"></div><strong>{}</strong><p>{}</p></div>", escape(name),escape(name),escape(name),escape(value)));
            }
        }
        html.push_str("</div></section><section id=\"system-recipes\"><h2>Components</h2><div class=\"examples\"><div><h3>Actions</h3><button class=\"ds-button\" data-system-recipe=\"button\">Continue</button> <button class=\"ds-button\" disabled>Unavailable</button></div><div><h3>Form fields</h3><label>Your name<input data-system-recipe=\"input\" class=\"ds-input\" placeholder=\"Alex Morgan\"></label></div><div role=\"button\" tabindex=\"0\" data-system-recipe=\"card\" class=\"ds-card\"><h3 class=\"ds-heading\">A place for related content</h3><p>A card using this system’s spacing, color and shape.</p></div></div></section><section id=\"system-foundations\"><h2>Foundations</h2><dl>");
        for (name, value) in &record.draft.tokens {
            if !name.contains("color") {
                html.push_str(&format!("<dt role=\"button\" tabindex=\"0\" data-system-token=\"{}\">{}</dt><dd>{}</dd>",escape(name),escape(name),escape(value)));
            }
        }
        html.push_str("</dl></section>");
    }
    if !record.sources.is_empty() {
        html.push_str("<section><h2>Sources and decisions</h2>");
        for (source, note) in &record.sources {
            html.push_str(&format!(
                "<p><strong>{}</strong><br>{}</p>",
                escape(source),
                escape(note)
            ));
        }
        html.push_str("</section>");
    }
    html.push_str("</main></body></html>");
    StudioDocument { html, js: String::new(), css: "body{margin:0;background:var(--color-background,#fff);color:var(--color-text,#202124);font-family:var(--font-body,system-ui);font-size:var(--font-size-body,16px);line-height:1.5}main{max-width:960px;margin:auto;padding:48px}header{margin-bottom:48px}header h1{font-size:36px;margin:0}header p{margin:8px 0}section{margin:40px 0}section>h2{font:600 18px system-ui;margin-bottom:24px}p{max-width:65ch}h3{margin-top:0}.type-sample{font-family:var(--font-body,system-ui)}.type-sample h1{font-size:var(--font-size-heading,32px)}.swatches{display:grid;grid-template-columns:repeat(auto-fit,minmax(170px,1fr));gap:24px}.swatch{height:92px;border-radius:var(--radius-control,6px);border:1px solid #8885;margin-bottom:12px}.swatches strong,.swatches p{font-size:13px;overflow-wrap:anywhere}.swatches p{margin:4px 0}.examples{display:grid;gap:32px}button,input{font:inherit;padding:var(--space-small,8px) var(--space-medium,16px);border:1px solid #8887;border-radius:var(--radius-control,6px)}button:disabled{opacity:.45}input{display:block;margin-top:8px}dl{display:grid;grid-template-columns:minmax(150px,1fr) 2fr;gap:12px}dd{margin:0;overflow-wrap:anywhere}dt{font-weight:500}@media(max-width:600px){main{padding:24px}dl{grid-template-columns:1fr}}[data-system-token],[data-system-recipe]{cursor:pointer}:focus-visible{outline:2px solid var(--color-primary,#335cff);outline-offset:3px}".into() }
}
