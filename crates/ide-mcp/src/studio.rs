use super::*;
use anyhow::ensure;
use ide_core::studio::*;

pub(super) fn tools() -> Vec<Box<dyn Tool>> {
    [
        "studio_context",
        "studio_project_read",
        "studio_import_propose",
        "studio_read",
        "studio_apply",
        "studio_snapshot",
        "studio_review",
        "studio_handoff_read",
    ]
    .into_iter()
    .map(|name| Box::new(StudioTool(name)) as Box<dyn Tool>)
    .collect()
}
pub(super) fn is_studio(ctx: &ServerContext) -> bool {
    ctx.studio
        || studio_store(ctx)
            .ok()
            .zip(ctx.agent_id)
            .is_some_and(|(store, id)| {
                store
                    .cache
                    .join("roles")
                    .join(format!("{id}.json"))
                    .exists()
            })
}
pub(super) fn allowed(name: &str) -> bool {
    matches!(
        name,
        "studio_project_read"
            | "studio_import_propose"
            | "studio_context"
            | "studio_read"
            | "studio_apply"
            | "studio_snapshot"
            | "studio_review"
            | "task_read"
            | "task_list"
            | "task_image"
            | "summary_read"
            | "summary_save"
    )
}
fn studio_store(ctx: &ServerContext) -> Result<StudioStore> {
    StudioStore::new(ctx.project()?.path, ctx.store()?.root())
}
fn binding(ctx: &ServerContext, store: &StudioStore) -> Result<StudioAgentContext> {
    let role: StudioAgentContext = serde_json::from_slice(&fs::read(
        store
            .cache
            .join("roles")
            .join(format!("{}.json", ctx.agent_id()?)),
    )?)?;
    Ok(role)
}
struct StudioTool(&'static str);
impl Tool for StudioTool {
    fn name(&self) -> &'static str {
        self.0
    }
    fn title(&self) -> &'static str {
        self.0
    }
    fn description(&self) -> &'static str {
        match self.0 {
            "studio_import_propose" => "Save results of an authorized From code analysis for native review. Does not create or publish systems. Provide summary and systems: [{name,platform,description,sources:{project_relative_file:evidence},system:{schema_version:1,revision:0,tokens,recipes,font_faces},assets?:{font_filename:project_relative_file},existing_system_id?:uuid}]. Recover distinct systems from real UI evidence; shared app themes are one system. Up to 16 systems; an empty list explains no UI found. Only From code agents can call this tool.",
            "studio_project_read" => "Read project context without shell access. Optional path is a contained project-relative file or directory (default root). Files return up to 200 lines starting at optional start_line. Directories return up to 200 children. No writes.",
            "studio_context" => "Read this Studio agent's exact design, current host-authorized turn scope, effective tokens, screen list, ordered sections, the frozen current section (if the user selected one), revision and fingerprint. Call before editing. Scope cannot be expanded through MCP.",
            "studio_read" => "Read exactly one of: an HTML screen (screen_id); a section (section_id) with its ordered screen metadata, archived status and documents; a bundled asset (asset_path); or all design source if none is given. Other screens are read-only unless listed in studio_context scope.",
            "studio_apply" => "Apply a StudioTransaction: id (fresh UUID/idempotency key), scope_id, design_id, expected_revision, expected_fingerprint, operations. Operations: add_asset {name,bytes} (new local image/font only, maximum 4 MB), write_screen {screen_id,document:{html,css,js}}, create_screen {screen:{id,name,width,height,archived,files:{html:index.html,css:styles.css,js:prototype.js}},document,section_id?} (omit section_id to use the frozen current section, null for unsectioned, or a section UUID), update_screen {screen}, reorder {screen_ids}, rename_design {name}, set_overrides {overrides:{tokens}}, set_source {document?:{reference,content},task?:{reference,content}}, create_section {section:{id,name,screen_ids?,direction?:horizontal|vertical,gap?,title_style?:left_title|full_width_header,header_alignment?:left|center}} (listed screens move into it), update_section {section_id,name?,direction?,gap?,title_style?,header_alignment?}, move_screen_to_section {screen_id,section_id (null = unsectioned),before_screen_id?}, reorder_section_screens {section_id,screen_ids}, reorder_sections {section_ids}, set_section_layout {direction:stacked|side_by_side,origin?:{x,y}}, ungroup_section {section_id} (keeps every screen), set_system {system,expected_system_revision} (system draft only), system_details {platform,sources:{relative_source_or_proposal:explanation}} (system workspace only). A system workspace has generated specimens: use set_system, not write_screen. Applying a shared system is a host review action. Only host-authorized operations succeed. On conflict read again, never overwrite newer edits.",
            "studio_snapshot" => "Request/read a screenshot at a saved revision and fingerprint (both required) of exactly one target: screen_id, or section_id for a paginated overview of that section's active screens in order (at most 12 per page; optional page from 1). A section overview supplements but never replaces per-screen snapshots and reviews of edited screens. If pending, retry after rendering; never claim visual review without the returned image.",
            "studio_review" => "Record a passed visual review after inspecting studio_snapshot for this screen at its current fingerprint. Provide concrete notes about layout, content and accessibility. Fix issues first. Completion is blocked until every changed screen passes review.",
            _ => "Read the immutable Studio implementation snapshot identified by handoff_id. It remains accessible from Solo worktrees and when source files are untracked. Optional screen_id returns that screen; section can be manifest, assets, tokens, or full (default manifest).",
        }
    }
    fn input_schema(&self) -> Value {
        match self.0 {
            "studio_import_propose" => {
                json!({"type":"object","properties":{"scope_id":{"type":"string","format":"uuid"},"summary":{"type":"string","maxLength":4000},"systems":{"type":"array","maxItems":16,"items":{"type":"object","properties":{"name":{"type":"string"},"platform":{"type":"string"},"description":{"type":"string"},"sources":{"type":"object","additionalProperties":{"type":"string"}},"system":{"type":"object"},"assets":{"type":"object","additionalProperties":{"type":"string"}},"existing_system_id":{"type":"string","format":"uuid"}},"required":["name","platform","description","sources","system"],"additionalProperties":false}}},"required":["scope_id","summary","systems"],"additionalProperties":false})
            }
            "studio_project_read" => {
                json!({"type":"object","properties":{"path":{"type":"string"},"start_line":{"type":"integer","minimum":1}},"additionalProperties":false})
            }
            "studio_context" => {
                json!({"type":"object","properties":{},"additionalProperties":false})
            }
            "studio_snapshot" => {
                json!({"type":"object","properties":{"screen_id":{"type":"string","format":"uuid"},"section_id":{"type":"string","format":"uuid"},"page":{"type":"integer","minimum":1},"revision":{"type":"integer","minimum":0},"fingerprint":{"type":"string"}},"required":["revision","fingerprint"],"additionalProperties":false})
            }
            "studio_review" => {
                json!({"type":"object","properties":{"screen_id":{"type":"string","format":"uuid"},"fingerprint":{"type":"string"},"notes":{"type":"string"}},"required":["screen_id","fingerprint","notes"],"additionalProperties":false})
            }
            "studio_read" => {
                json!({"type":"object","properties":{"screen_id":{"type":"string","format":"uuid"},"section_id":{"type":"string","format":"uuid"},"asset_path":{"type":"string"}},"additionalProperties":false})
            }
            "studio_apply" => {
                json!({"type":"object","properties":{"transaction":{"type":"object","properties":{"id":{"type":"string"},"scope_id":{"type":"string"},"design_id":{"type":"string"},"expected_revision":{"type":"integer"},"expected_fingerprint":{"type":"string"},"operations":{"type":"array","items":{"type":"object"}}},"required":["id","scope_id","design_id","expected_revision","expected_fingerprint","operations"]}},"required":["transaction"],"additionalProperties":false})
            }
            _ => {
                json!({"type":"object","properties":{"handoff_id":{"type":"string","format":"uuid"},"asset_path":{"type":"string"},"screen_id":{"type":"string","format":"uuid"},"section":{"enum":["manifest","assets","tokens","full"]}},"required":["handoff_id"],"additionalProperties":false})
            }
        }
    }
    fn call(&self, ctx: &ServerContext, args: &Value) -> Result<Vec<Value>> {
        let store = studio_store(ctx)?;
        if self.0 == "studio_handoff_read" {
            let id = parse_id(args, "handoff_id")?;
            let snapshot = store.read_handoff(id)?;
            let snapshot_path = store.cache.join("handoffs").join(format!("{id}.json"));
            let result = if let Some(path) = args.get("asset_path").and_then(Value::as_str) {
                json!({"path":path,"base64":base64::engine::general_purpose::STANDARD.encode(snapshot.assets.get(path).context("Asset is not in this handoff")?)})
            } else if args.get("screen_id").is_some() {
                serde_json::to_value(
                    snapshot
                        .design
                        .documents
                        .get(&parse_id(args, "screen_id")?)
                        .context("Screen is not in this handoff")?,
                )?
            } else {
                match args
                    .get("section")
                    .and_then(Value::as_str)
                    .unwrap_or("manifest")
                {
                    "assets" => {
                        json!({"snapshot_path":snapshot_path,"assets":snapshot.assets.iter().map(|(path,bytes)|json!({"path":path,"size":bytes.len()})).collect::<Vec<_>>(),"usage":"Copy asset bytes from this immutable local JSON bundle, or pass asset_path to studio_handoff_read for base64 content."})
                    }
                    "tokens" => {
                        json!({"system":snapshot.design_system_context,"tokens":snapshot.design.tokens(),"recipes":snapshot.design.system.recipes,"font_faces":snapshot.design.system.font_faces})
                    }
                    "full" => serde_json::to_value(&snapshot)?,
                    _ => {
                        json!({
                            "handoff_id": snapshot.id,
                            "snapshot_path": snapshot_path,
                            "manifest": snapshot.design.manifest,
                            "design_system_context": snapshot.design_system_context,
                            "design_system_usage": "Read section=tokens for this snapshot's effective tokens, component recipes, and fonts. When a design system is selected, use that captured system together with screen-local styles and overrides; do not substitute a generic theme or a newer system revision. Otherwise use the captured local styles without attaching an unrelated system.",
                            "screen_ids": snapshot.screen_ids,
                            "instruction": snapshot.instruction,
                            "fingerprint": snapshot.design.fingerprint,
                            "asset_paths": snapshot.assets.keys().collect::<Vec<_>>(),
                        })
                    }
                }
            };
            return Ok(vec![text_content(result.to_string())]);
        }
        let role = binding(ctx, &store)?;
        let scope = store.scope(ctx.agent_id()?)?;
        ensure!(
            scope.design_id == role.design_id,
            "Studio role and scope do not match"
        );
        if role.target == StudioAgentTarget::DesignSystemImport {
            match self.0 {
                "studio_context" => return Ok(vec![text_content(store.request_context(ctx.agent_id()?)?.to_string())]),
                "studio_import_propose" => {
                    ensure!(serde_json::to_vec(args)?.len() <= 1024 * 1024, "Import proposal is too large");
                    let proposal = store.propose_code_systems(
                        ctx.agent_id()?, parse_id(args, "scope_id")?, args["summary"].as_str().context("Missing summary")?.to_owned(),
                        serde_json::from_value(args["systems"].clone())?,
                    )?;
                    return Ok(vec![text_content(json!({"proposal_id":proposal.proposal_id,"systems":proposal.candidates.len(),"status":"Ready for the user to select and create drafts. No system has been created or applied."}).to_string())]);
                }
                "studio_project_read" => {},
                _ => anyhow::bail!("From code analysis can only read project files and propose systems"),
            }
        } else {
            ensure!(self.0 != "studio_import_propose", "Start From code in the design-system library before proposing systems");
        }
        if self.0 == "studio_project_read" {
            let relative = args.get("path").and_then(Value::as_str).unwrap_or("");
            let path = if relative.is_empty() {
                store.project.clone()
            } else {
                contained(&store.project, relative)?
            };
            let result = if path.is_dir() {
                let mut names = fs::read_dir(path)?
                    .filter_map(|e| e.ok())
                    .filter(|e| {
                        !matches!(
                            e.file_name().to_str(),
                            Some(".git" | "node_modules" | "target")
                        )
                    })
                    .take(200)
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect::<Vec<_>>();
                names.sort();
                json!({"entries":names})
            } else {
                ensure!(
                    fs::metadata(&path)?.len() <= 4 * 1024 * 1024,
                    "File is too large for Studio context"
                );
                let text = fs::read_to_string(path)?;
                if role.target == StudioAgentTarget::DesignSystemImport {
                    store.record_code_import_read(ctx.agent_id()?, relative, text.as_bytes())?;
                }
                let start = args
                    .get("start_line")
                    .and_then(Value::as_u64)
                    .unwrap_or(1)
                    .max(1) as usize;
                json!({"path":relative,"start_line":start,"text":text.lines().skip(start-1).take(200).collect::<Vec<_>>().join("\n")})
            };
            return Ok(vec![text_content(result.to_string())]);
        }
        ensure!(
            store.is_system_workspace(role.design_id)?
                == (role.target == StudioAgentTarget::DesignSystem),
            "Studio target kind does not match the bound workspace"
        );
        let design = if self.0 == "studio_snapshot" {
            store
                .saved_revision(
                    role.design_id,
                    args["revision"]
                        .as_u64()
                        .context("Missing saved revision")?,
                    args["fingerprint"]
                        .as_str()
                        .context("Missing saved fingerprint")?,
                )?
                .design
        } else {
            store.load(role.design_id)?
        };
        let result = match self.0 {
            "studio_context" => {
                let request_context = store.request_context(ctx.agent_id()?)?;
                let sections = design
                    .manifest
                    .sections
                    .iter()
                    .map(|section| section_summary(&design.manifest, section))
                    .collect::<Vec<_>>();
                json!({"current_section":request_context["current_section"].clone(),"request_context":request_context,"scope":scope,"overrides":design.overrides,"manifest":design.manifest,"sections":sections,"section_layout":design.manifest.section_layout,"fingerprint":design.fingerprint,"tokens":design.tokens(),"recipes":design.system.recipes,"asset_paths":store.assets(role.design_id)?.keys().collect::<Vec<_>>()})
            }
            "studio_read" => {
                let selector = single_selector(args, &["screen_id", "section_id", "asset_path"])?;
                if selector == Some("section_id") {
                    let id = parse_id(args, "section_id")?;
                    let section = design
                        .manifest
                        .section(id)
                        .context("Section does not exist")?;
                    let documents = section
                        .screen_ids
                        .iter()
                        .filter_map(|id| design.documents.get(id).map(|doc| (id, doc)))
                        .map(|(id, doc)| json!({"screen_id":id,"document":doc}))
                        .collect::<Vec<_>>();
                    json!({"section":section_summary(&design.manifest, section),"documents":documents,"revision":design.manifest.revision,"fingerprint":design.fingerprint})
                } else if let Some(path) = args.get("asset_path").and_then(Value::as_str) {
                    let assets = store.assets(role.design_id)?;
                    let bytes = assets.get(path).context("Asset does not exist")?;
                    json!({"path":path,"base64":base64::engine::general_purpose::STANDARD.encode(bytes)})
                } else if args.get("screen_id").is_some() {
                    {
                        let id = parse_id(args, "screen_id")?;
                        if design.documents.contains_key(&id) {
                            store.record_focus(ctx.agent_id()?, scope.id, role.design_id, id)?;
                        }
                        let recovered =
                            fs::read(store.cache.join("drafts").join(format!("{id}.json")))
                                .ok()
                                .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                                .filter(|d| d["recoverable"] == true);
                        json!({"document":design.documents.get(&id).context("Screen does not exist")?,"recovered_draft":recovered})
                    }
                } else {
                    serde_json::to_value(design)?
                }
            }
            "studio_apply" => {
                let transaction: StudioTransaction = serde_json::from_value(
                    args.get("transaction")
                        .cloned()
                        .context("Missing transaction")?,
                )?;
                let result = store.apply_for_agent(ctx.agent_id()?, &transaction)?;
                if let Some(StudioOperation::WriteScreen { screen_id, .. }) = transaction
                    .operations
                    .iter()
                    .rev()
                    .find(|op| matches!(op, StudioOperation::WriteScreen { .. }))
                {
                    store.record_focus(ctx.agent_id()?, scope.id, role.design_id, *screen_id)?;
                }
                json!({"revision":result.manifest.revision,"fingerprint":result.fingerprint,"transaction_id":transaction.id})
            }
            "studio_review" => {
                let reviewed = parse_id(args, "screen_id")?;
                store.review_screen(
                    ctx.agent_id()?,
                    reviewed,
                    args["fingerprint"]
                        .as_str()
                        .context("Missing fingerprint")?,
                    args["notes"].as_str().context("Missing review notes")?,
                )?;
                store.clear_focus(ctx.agent_id()?, reviewed);
                json!({"review":"passed"})
            }
            "studio_snapshot" => {
                match single_selector(args, &["screen_id", "section_id"])? {
                    Some("section_id") => return section_overview(&store, &design, args),
                    Some(_) => {}
                    None => anyhow::bail!("Pass screen_id or section_id"),
                }
                let id = parse_id(args, "screen_id")?;
                ensure!(design.documents.contains_key(&id), "Screen does not exist");
                store.request_thumbnail(&design, id)?;
                let bytes = fs::read(store.thumbnail_path(&design, id)).context(
                    "Screenshot is still pending for this revision. Open Studio to render it.",
                )?;
                store.record_snapshot_view(ctx.agent_id()?, &design, id)?;
                return Ok(vec![
                    text_content(format!(
                        "Screen {id}, revision {}, fingerprint {}",
                        design.manifest.revision, design.fingerprint
                    )),
                    image_content(
                        "image/png",
                        &base64::engine::general_purpose::STANDARD.encode(bytes),
                    ),
                ]);
            }
            _ => unreachable!(),
        };
        Ok(vec![text_content(result.to_string())])
    }
}
/// At most one target selector; ambiguous combinations are rejected.
fn single_selector(args: &Value, names: &[&'static str]) -> Result<Option<&'static str>> {
    let present = names
        .iter()
        .copied()
        .filter(|name| args.get(*name).is_some_and(|v| !v.is_null()))
        .collect::<Vec<_>>();
    ensure!(
        present.len() <= 1,
        "Ambiguous Studio target: pass only one of {}",
        names.join(", ")
    );
    Ok(present.first().copied())
}
const OVERVIEW_PAGE: usize = 12;
/// Ordered overview images of a section's active screens at an exact saved
/// revision. It supplements, and never records, per-screen visual reviews.
fn section_overview(store: &StudioStore, design: &StudioDesign, args: &Value) -> Result<Vec<Value>> {
    let id = parse_id(args, "section_id")?;
    let section = design
        .manifest
        .section(id)
        .context("Section does not exist in this revision")?;
    let active = section
        .screen_ids
        .iter()
        .filter_map(|id| design.manifest.screens.iter().find(|s| s.id == *id))
        .filter(|s| !s.archived)
        .collect::<Vec<_>>();
    let pages = active.len().div_ceil(OVERVIEW_PAGE).max(1);
    let page = args.get("page").and_then(Value::as_u64).unwrap_or(1) as usize;
    ensure!((1..=pages).contains(&page), "Page must be between 1 and {pages}");
    let mut images = Vec::new();
    let mut listed = Vec::new();
    for (index, screen) in active
        .iter()
        .enumerate()
        .skip((page - 1) * OVERVIEW_PAGE)
        .take(OVERVIEW_PAGE)
    {
        store.request_thumbnail(design, screen.id)?;
        let status = match fs::read(store.thumbnail_path(design, screen.id)) {
            Ok(bytes) => {
                images.push(image_content(
                    "image/png",
                    &base64::engine::general_purpose::STANDARD.encode(bytes),
                ));
                format!("image {}", images.len())
            }
            Err(_) => "pending".to_string(),
        };
        listed.push(json!({"order":index + 1,"screen_id":screen.id,"name":screen.name,"width":screen.width,"height":screen.height,"status":status}));
    }
    let summary = json!({
        "section_id": section.id,
        "name": section.name,
        "direction": section.direction,
        "revision": design.manifest.revision,
        "fingerprint": design.fingerprint,
        "page": page,
        "pages": pages,
        "active_screens": active.len(),
        "screens": listed,
        "note": "Overview only. Edited screens still need studio_snapshot by screen_id and studio_review. Retry pending images after Studio renders them."
    });
    let mut content = vec![text_content(summary.to_string())];
    content.extend(images);
    Ok(content)
}
fn parse_id(args: &Value, name: &str) -> Result<uuid::Uuid> {
    Ok(args
        .get(name)
        .and_then(Value::as_str)
        .context(format!("Missing {name}"))?
        .parse()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn from_code_agent_can_read_and_propose_but_cannot_write_designs() {
        let temp = tempfile::tempdir().unwrap();
        let local = LocalStore::open(temp.path().join("store")).unwrap();
        let root = temp.path().join("project");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("theme.css"), ":root { --color-primary: #123456; }").unwrap();
        let project = ide_core::Project::from_path(root.clone());
        let mut config = ide_core::AppConfig::default();
        config.projects.push(project.clone());
        local.save_workspace_config(&config).unwrap();
        let store = StudioStore::new(root.clone(), local.root()).unwrap();
        let import = store.create_code_import().unwrap();
        let agent = uuid::Uuid::new_v4();
        let role = StudioAgentContext { target: StudioAgentTarget::DesignSystemImport, design_id: import.id, conversation_id: import.id };
        store.prepare_code_import(agent, &role).unwrap();
        let ctx = ServerContext { review_run: None, studio: true, delegation_scope: None, project_id: Some(project.id.0), agent_id: Some(agent), store: Some(local) };
        let registry = ToolRegistry::default();
        let read = registry.call(&ctx, &json!({"name":"studio_project_read","arguments":{"path":"theme.css"}}));
        assert_ne!(read["isError"], true);
        assert!(read["content"][0]["text"].as_str().unwrap().contains("#123456"));
        let scope = store.scope(agent).unwrap();
        let proposal = registry.call(&ctx, &json!({"name":"studio_import_propose","arguments":{
            "scope_id":scope.id,"summary":"Found the web theme.","systems":[{
                "name":"Web","platform":"Web","description":"One shared theme.",
                "sources":{"theme.css":"Defines the primary color."},
                "system":{"schema_version":1,"revision":0,"tokens":{"color-primary":"#123456"},"recipes":{}}
            }]
        }}));
        assert_ne!(proposal["isError"], true, "{proposal}");
        assert!(store.systems().unwrap().is_empty());
        for name in ["studio_apply", "studio_snapshot", "studio_read", "create_choro_doc"] {
            let denied = registry.call(&ctx, &json!({"name":name,"arguments":{}}));
            assert_eq!(denied["isError"], true, "{name}: {denied}");
        }
        assert_eq!(fs::read_to_string(root.join("theme.css")).unwrap(), ":root { --color-primary: #123456; }");
    }

    #[test]
    fn screenshot_schema_requires_exact_revision_and_review_is_scoped() {
        let schema = StudioTool("studio_snapshot").input_schema();
        assert_eq!(schema["required"], json!(["revision", "fingerprint"]));
        assert!(schema["properties"]["screen_id"].is_object());
        assert!(schema["properties"]["section_id"].is_object());
        assert_eq!(schema["additionalProperties"], false);
        assert!(allowed("studio_review"));
        assert!(!allowed("studio_handoff_read"));
        let names = tools().iter().map(|t| t.name()).collect::<Vec<_>>();
        assert!(names.contains(&"studio_review"));
    }
    fn design_agent(sections: usize, screens: usize) -> (tempfile::TempDir, ServerContext, StudioStore, StudioDesign, uuid::Uuid) {
        let temp = tempfile::tempdir().unwrap();
        let local = LocalStore::open(temp.path().join("store")).unwrap();
        let root = temp.path().join("project");
        fs::create_dir_all(&root).unwrap();
        let project = ide_core::Project::from_path(root.clone());
        let mut config = ide_core::AppConfig::default();
        config.projects.push(project.clone());
        local.save_workspace_config(&config).unwrap();
        let store = StudioStore::new(root, local.root()).unwrap();
        let design = store.create("Flows").unwrap();
        let scope = StudioTurnScope::whole_design(&design);
        let created = (0..screens)
            .map(|i| StudioScreen { id: uuid::Uuid::new_v4(), name: format!("Step {}", i + 1), width: 390, height: 844, archived: i == 1, files: Default::default() })
            .collect::<Vec<_>>();
        let mut operations = created
            .iter()
            .map(|screen| StudioOperation::CreateScreen { screen: screen.clone(), document: starter_document(), section_id: Some(None) })
            .collect::<Vec<_>>();
        for i in 0..sections {
            operations.push(StudioOperation::CreateSection {
                section: StudioSection {
                    screen_ids: if i == 0 { created.iter().map(|s| s.id).collect() } else { vec![] },
                    ..StudioSection::new(format!("Flow {i}"))
                },
            });
        }
        let design = store
            .apply(&scope, &StudioTransaction { id: uuid::Uuid::new_v4(), scope_id: scope.id, design_id: design.manifest.id, expected_revision: design.manifest.revision, expected_fingerprint: design.fingerprint.clone(), operations })
            .unwrap();
        let agent = uuid::Uuid::new_v4();
        let section = design.manifest.sections[0].id;
        store.save_scope(agent, &scope_for_section_request(&design, section)).unwrap();
        ide_core::studio::atomic(
            &store.cache.join("roles").join(format!("{agent}.json")),
            &serde_json::to_vec(&StudioAgentContext { target: StudioAgentTarget::Design, design_id: design.manifest.id, conversation_id: uuid::Uuid::new_v4() }).unwrap(),
        )
        .unwrap();
        let ctx = ServerContext { review_run: None, studio: true, delegation_scope: None, project_id: Some(project.id.0), agent_id: Some(agent), store: Some(local) };
        (temp, ctx, store, design, section)
    }

    #[test]
    fn section_reads_and_overviews_are_ordered_paginated_and_unambiguous() {
        let (_temp, ctx, store, design, section) = design_agent(2, 14);
        let registry = ToolRegistry::default();
        let context = registry.call(&ctx, &json!({"name":"studio_context","arguments":{}}));
        let context: Value = serde_json::from_str(context["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(context["current_section"]["id"], json!(section));
        assert_eq!(context["sections"].as_array().unwrap().len(), 2);
        let read = registry.call(&ctx, &json!({"name":"studio_read","arguments":{"section_id":section}}));
        let read: Value = serde_json::from_str(read["content"][0]["text"].as_str().unwrap()).unwrap();
        let members = &design.manifest.sections[0].screen_ids;
        assert_eq!(read["section"]["screen_ids"], json!(members));
        assert_eq!(read["section"]["screens"][1]["archived"], true);
        assert_eq!(read["documents"].as_array().unwrap().len(), 14);
        for ambiguous in [
            json!({"name":"studio_read","arguments":{"section_id":section,"screen_id":members[0]}}),
            json!({"name":"studio_read","arguments":{"section_id":section,"asset_path":"x.png"}}),
            json!({"name":"studio_snapshot","arguments":{"section_id":section,"screen_id":members[0],"revision":design.manifest.revision,"fingerprint":design.fingerprint}}),
            json!({"name":"studio_snapshot","arguments":{"revision":design.manifest.revision,"fingerprint":design.fingerprint}}),
        ] {
            let result = registry.call(&ctx, &ambiguous);
            assert_eq!(result["isError"], true, "{ambiguous}");
        }
        // Rendered screens arrive as images; others are reported pending.
        let png = b"\x89PNG\r\n\x1a\nfixture".to_vec();
        for id in members.iter().take(3) {
            let path = store.thumbnail_path(&design, *id);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, &png).unwrap();
        }
        let snapshot = |page: u64| registry.call(&ctx, &json!({"name":"studio_snapshot","arguments":{"section_id":section,"page":page,"revision":design.manifest.revision,"fingerprint":design.fingerprint}}));
        let first = snapshot(1);
        assert_ne!(first["isError"], true, "{first}");
        let summary: Value = serde_json::from_str(first["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(summary["active_screens"], 13, "archived members take no overview slot");
        assert_eq!(summary["pages"], 2);
        assert_eq!(summary["screens"].as_array().unwrap().len(), 12);
        assert_eq!(summary["screens"][0]["status"], "image 1");
        assert_eq!(summary["screens"][1]["screen_id"], json!(members[2]));
        assert_eq!(summary["screens"][11]["status"], "pending");
        assert_eq!(first["content"].as_array().unwrap().len(), 1 + 2);
        let second: Value = serde_json::from_str(snapshot(2)["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(second["screens"].as_array().unwrap().len(), 1);
        assert_eq!(second["screens"][0]["order"], 13);
        assert_eq!(snapshot(3)["isError"], true);
        // An overview never counts as a per-screen review view.
        let review = registry.call(&ctx, &json!({"name":"studio_review","arguments":{"screen_id":members[0],"fingerprint":design.fingerprint,"notes":"Looks right."}}));
        assert_eq!(review["isError"], true);
    }

    #[test]
    fn agents_group_screens_through_studio_apply() {
        let (_temp, ctx, store, design, section) = design_agent(1, 2);
        let registry = ToolRegistry::default();
        let scope = store.scope(ctx.agent_id.unwrap()).unwrap();
        let members = design.manifest.sections[0].screen_ids.clone();
        let created = uuid::Uuid::new_v4();
        let result = registry.call(&ctx, &json!({"name":"studio_apply","arguments":{"transaction":{
            "id": uuid::Uuid::new_v4(), "scope_id": scope.id, "design_id": design.manifest.id,
            "expected_revision": design.manifest.revision, "expected_fingerprint": design.fingerprint,
            "operations": [
                {"operation":"create_section","section":{"id":created,"name":"Add images","direction":"vertical","title_style":"full_width_header","header_alignment":"center"}},
                {"operation":"move_screen_to_section","screen_id":members[1],"section_id":created},
                {"operation":"reorder_sections","section_ids":[created, section]},
                {"operation":"set_section_layout","direction":"side_by_side"},
                {"operation":"create_screen","screen":{"id":uuid::Uuid::new_v4(),"name":"Pick","width":390,"height":844,"archived":false,"files":{"html":"index.html","css":"styles.css","js":"prototype.js"}},"document":{"html":"<h1>Pick</h1>","css":"","js":""}}
            ]
        }}}));
        assert_ne!(result["isError"], true, "{result}");
        let after = store.load(design.manifest.id).unwrap();
        assert_eq!(after.manifest.sections[0].id, created);
        assert_eq!(after.manifest.sections[0].title_style, StudioSectionTitleStyle::FullWidthHeader);
        assert_eq!(after.manifest.section_layout.direction, StudioSectionArrangement::SideBySide);
        // Omitted section_id uses the frozen current section.
        assert_eq!(after.manifest.sections[1].screen_ids.len(), 2);
        assert_eq!(after.manifest.sections[1].screen_ids[0], members[0]);
    }

    #[test]
    fn role_restrictions_fail_closed_even_without_a_project_store() {
        let ctx = ServerContext {
            review_run: None,
            studio: true,
            delegation_scope: None,
            project_id: None,
            agent_id: None,
            store: None,
        };
        let registry = ToolRegistry::default();
        let names = registry
            .list_for(&ctx)
            .into_iter()
            .map(|v| v["name"].as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        assert!(names.contains(&"studio_apply".to_string()));
        assert!(!names.contains(&"create_choro_doc".to_string()));
        assert!(!names.iter().any(|name| name.starts_with("delegation_")));
        for name in [
            "create_choro_doc",
            "preview_open",
            "delegation_start",
            "studio_handoff_read",
        ] {
            let result = registry.call(&ctx, &json!({"name":name,"arguments":{}}));
            assert_eq!(result["isError"], true);
            assert!(result["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("unavailable"));
        }
    }
}
