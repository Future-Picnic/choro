use super::*;
use anyhow::ensure;
use ide_core::studio::*;

pub(super) fn tools() -> Vec<Box<dyn Tool>> {
    [
        "studio_context",
        "studio_project_read",
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
            "studio_project_read" => "Read project context without shell access. Optional path is a contained project-relative file or directory (default root). Files return up to 200 lines starting at optional start_line. Directories return up to 200 children. No writes.",
            "studio_context" => "Read this Studio agent's exact design, current host-authorized turn scope, effective tokens, screen list, revision and fingerprint. Call before editing. Scope cannot be expanded through MCP.",
            "studio_read" => "Read an HTML screen (screen_id), a bundled asset (asset_path), or all design source if omitted. Other screens are read-only unless listed in studio_context scope.",
            "studio_apply" => "Apply a StudioTransaction: id (fresh UUID/idempotency key), scope_id, design_id, expected_revision, expected_fingerprint, operations. Operations: add_asset {name,bytes} (new local image/font only, maximum 4 MB), write_screen {screen_id,document:{html,css,js}}, create_screen {screen:{id,name,width,height,archived,files:{html:index.html,css:styles.css,js:prototype.js}},document}, update_screen {screen}, reorder {screen_ids}, rename_design {name}, set_overrides {overrides:{tokens}}, set_source {document?:{reference,content},task?:{reference,content}}, set_system {system,expected_system_revision} (system draft only), system_details {platform,sources:{relative_source_or_proposal:explanation}} (system workspace only). A system workspace has generated specimens: use set_system, not write_screen. Applying a shared system is a host review action. Only host-authorized operations succeed. On conflict read again, never overwrite newer edits.",
            "studio_snapshot" => "Request/read a screenshot of screen_id at a saved revision and fingerprint. Both revision and fingerprint are required. If pending, retry after rendering; never claim visual review without the returned image.",
            "studio_review" => "Record a passed visual review after inspecting studio_snapshot for this screen at its current fingerprint. Provide concrete notes about layout, content and accessibility. Fix issues first. Completion is blocked until every changed screen passes review.",
            _ => "Read the immutable Studio implementation snapshot identified by handoff_id. It remains accessible from Solo worktrees and when source files are untracked. Optional screen_id returns that screen; section can be manifest, assets, tokens, or full (default manifest).",
        }
    }
    fn input_schema(&self) -> Value {
        match self.0 {
            "studio_project_read" => {
                json!({"type":"object","properties":{"path":{"type":"string"},"start_line":{"type":"integer","minimum":1}},"additionalProperties":false})
            }
            "studio_context" => {
                json!({"type":"object","properties":{},"additionalProperties":false})
            }
            "studio_snapshot" => {
                json!({"type":"object","properties":{"screen_id":{"type":"string","format":"uuid"},"revision":{"type":"integer","minimum":0},"fingerprint":{"type":"string"}},"required":["screen_id","revision","fingerprint"],"additionalProperties":false})
            }
            "studio_review" => {
                json!({"type":"object","properties":{"screen_id":{"type":"string","format":"uuid"},"fingerprint":{"type":"string"},"notes":{"type":"string"}},"required":["screen_id","fingerprint","notes"],"additionalProperties":false})
            }
            "studio_read" => {
                json!({"type":"object","properties":{"screen_id":{"type":"string","format":"uuid"},"asset_path":{"type":"string"}},"additionalProperties":false})
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
                        json!({"handoff_id":snapshot.id,"snapshot_path":snapshot_path,"manifest":snapshot.design.manifest,"screen_ids":snapshot.screen_ids,"instruction":snapshot.instruction,"fingerprint":snapshot.design.fingerprint,"asset_paths":snapshot.assets.keys().collect::<Vec<_>>()})
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
                json!({"request_context":store.request_context(ctx.agent_id()?)?,"scope":scope,"overrides":design.overrides,"manifest":design.manifest,"fingerprint":design.fingerprint,"tokens":design.tokens(),"recipes":design.system.recipes,"asset_paths":store.assets(role.design_id)?.keys().collect::<Vec<_>>()})
            }
            "studio_read" => {
                if let Some(path) = args.get("asset_path").and_then(Value::as_str) {
                    let assets = store.assets(role.design_id)?;
                    let bytes = assets.get(path).context("Asset does not exist")?;
                    json!({"path":path,"base64":base64::engine::general_purpose::STANDARD.encode(bytes)})
                } else if args.get("screen_id").is_some() {
                    {
                        let id = parse_id(args, "screen_id")?;
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
                json!({"revision":result.manifest.revision,"fingerprint":result.fingerprint,"transaction_id":transaction.id})
            }
            "studio_review" => {
                store.review_screen(
                    ctx.agent_id()?,
                    parse_id(args, "screen_id")?,
                    args["fingerprint"]
                        .as_str()
                        .context("Missing fingerprint")?,
                    args["notes"].as_str().context("Missing review notes")?,
                )?;
                json!({"review":"passed"})
            }
            "studio_snapshot" => {
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
    fn screenshot_schema_requires_exact_revision_and_review_is_scoped() {
        let schema = StudioTool("studio_snapshot").input_schema();
        assert_eq!(
            schema["required"],
            json!(["screen_id", "revision", "fingerprint"])
        );
        assert_eq!(schema["additionalProperties"], false);
        assert!(allowed("studio_review"));
        assert!(!allowed("studio_handoff_read"));
        let names = tools().iter().map(|t| t.name()).collect::<Vec<_>>();
        assert!(names.contains(&"studio_review"));
    }
    #[test]
    fn role_restrictions_fail_closed_even_without_a_project_store() {
        let ctx = ServerContext {
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
