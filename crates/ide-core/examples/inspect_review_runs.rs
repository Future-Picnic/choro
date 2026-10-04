//! Bounded diagnostics that open the live store strictly read-only.
//! Optional --replay prepares a new app-data snapshot from recorded receipts;
//! it never starts a reviewer or changes the live store or repository.
//! Use the embedded Turso engine; never open the live store with SQLite.
use anyhow::{Context, Result};
use serde_json::{json, Value};

fn main() -> Result<()> {
    tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(inspect())
}

async fn inspect() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let database = args.next().context("Usage: inspect_review_runs <database> <conversation or run UUID> [--replay | --export-run | --artifacts]")?;
    let parent = args.next().context("Missing conversation UUID")?;
    let flag = args.next();
    let replay = flag.as_deref() == Some("--replay");
    let export = flag.as_deref() == Some("--export-run");
    let artifacts = flag.as_deref() == Some("--artifacts");
    anyhow::ensure!(flag.is_none() || replay || export || artifacts, "Only --replay, --export-run or --artifacts is supported");
    let database = turso::Builder::new_local(&database).read_only(true).build().await?;
    let connection = database.connect()?;
    if artifacts {
        let mut rows = connection.query(
            "SELECT kind,payload_json FROM chat_timeline_events WHERE agent_id=?1 AND kind IN ('changed_files','proposed_plan') ORDER BY sequence",
            [parent.as_str()],
        ).await?;
        let mut paths = std::collections::BTreeSet::new();
        let mut receipt_count = 0;
        let mut latest_plan = Value::Null;
        while let Some(row) = rows.next().await? {
            let value: Value = serde_json::from_str(&row.get::<String>(1)?)?;
            if row.get::<String>(0)? == "proposed_plan" {
                latest_plan = json!({"id":value["id"],"markdown_bytes":value["markdown"].as_str().map(str::len),"implemented_at":value["implemented_at"]});
            } else if value["attribution_version"].as_u64().unwrap_or(0) > 0 {
                receipt_count += 1;
                for file in value["files"].as_array().into_iter().flatten() {
                    if let Some(path) = file["path"].as_str() { paths.insert(path.to_string()); }
                }
            }
        }
        println!("{}", json!({"confirmed_history_paths":paths.len(),"receipt_count":receipt_count,"paths":paths,"latest_plan":latest_plan}));
        return Ok(());
    }
    let mut rows = connection.query(
        "SELECT payload FROM code_review_runs WHERE parent_id=?1 OR id=?1 ORDER BY rowid DESC LIMIT 5",
        [parent.as_str()],
    ).await?;
    while let Some(row) = rows.next().await? {
        let run: Value = serde_json::from_str(&row.get::<String>(0)?)?;
        if export { println!("{}",run); return Ok(()); }
        println!("{}", json!({
            "id":run["id"], "provider":run["provider"], "model":run["model"],
            "state":run["state"], "stage":run["stage"], "revision":run["revision"],
            "started_at":run["started_at"], "finished_at":run["finished_at"],
            "files":run["files"].as_array().map(Vec::len),
            "unique_files":run["files"].as_array().map(|files| files.iter().map(|f| f["path"].to_string()).collect::<std::collections::BTreeSet<_>>().len()),
            "completed_files":run["files"].as_array().map(|files| files.iter().map(|f| f["path"].to_string()).collect::<std::collections::BTreeSet<_>>().iter().filter(|path| files.iter().filter(|f| f["path"].to_string() == **path).all(|f| f["status"] == "Complete")).count()),
            "findings":run["findings"].as_array().map(Vec::len),
            "limitations_count":run["limitations"].as_array().map(Vec::len),
            "limitations":run["limitations"].as_array().map(|items| items.iter().rev().take(3).collect::<Vec<_>>()),
            "freshness":run["freshness"]
        }));
    }
    let mut ledger = connection.query("SELECT path,baseline_content,result_content,segments_json FROM chat_file_ledger WHERE agent_id=?1 ORDER BY path LIMIT 80", [parent.as_str()]).await?;
    while let Some(row) = ledger.next().await? {
        let segments: Value = serde_json::from_str(&row.get::<String>(3)?).unwrap_or(Value::Null);
        println!("{}", json!({"ledger_path":row.get::<String>(0)?,
            "before_bytes":row.get::<Option<String>>(1)?.map(|s| s.len()),
            "after_bytes":row.get::<Option<String>>(2)?.map(|s| s.len()),
            "segments":segments.as_array().map(|s| s.iter().map(|v| json!({"keys":v.as_object().map(|o| o.keys().collect::<Vec<_>>()), "action_id":v["action_id"], "turn_id":v["turn_id"], "call_id":v["call_id"], "before_bytes":v["baseline_content"].as_str().map(str::len), "after_bytes":v["result_content"].as_str().map(str::len)})).collect::<Vec<_>>())}));
    }
    if replay {
        use ide_core::{agent_changes::MutationEvidence, code_review::*, AppConfig};
        let mut rows = connection.query(
            "SELECT payload FROM mutation_evidence WHERE agent_id=?1 ORDER BY sequence LIMIT 8193",
            [parent.as_str()],
        ).await?;
        let mut evidence = Vec::<MutationEvidence>::new();
        let mut bytes = 0usize;
        while let Some(row) = rows.next().await? {
            let raw = row.get::<String>(0)?;
            bytes += raw.len();
            anyhow::ensure!(bytes <= MAX_REVIEW_BYTES && evidence.len() < 8192,
                "Diagnostic receipt scope exceeds review limits");
            evidence.push(serde_json::from_str(&raw)?);
        }
        let first = evidence.first().context("No recorded receipts")?;
        let root = first.key.root.clone();
        let project = first.key.project_id;
        let agent = first.key.agent_id;
        let mut ledger = connection.query("SELECT path,attribution,baseline_hash,result_hash,baseline_content,result_content,updated_at,segments_json FROM chat_file_ledger WHERE agent_id=?1 ORDER BY path", [parent.as_str()]).await?;
        while let Some(row) = ledger.next().await? {
            let entry = ide_core::local_store::StoredChatFileLedgerEntry { agent_id:agent,path:row.get::<String>(0)?.into(),
                observed:row.get::<String>(1)? == "observed",baseline_hash:row.get::<Option<String>>(2)?,result_hash:row.get::<Option<String>>(3)?,
                baseline_content:row.get::<Option<String>>(4)?,result_content:row.get::<Option<String>>(5)?,updated_at:row.get::<i64>(6)? as u64,
                segments_json:row.get::<String>(7)?,additions:0,deletions:0,counts_unavailable:false };
            let recovered = confirmed_ledger_evidence(&entry, project, &root, &evidence);
            evidence.extend(recovered);
        }
        let mut run = ReviewRun::new(project, agent,
            "Diagnostic".into(), "No model request".into(), "None".into(), review_now());
        let storage = AppConfig::config_root().join("data/review-provider-checks")
            .join(run.id.to_string()).join("receipt-replay");
        let input = prepare_review(&mut run, &root, &storage,
            ReviewRequirements { user_requirements:vec![], decisions:vec![], checks:vec![],
                project_rules:vec![], supplementary_guidance:String::new() },
            &evidence, &["Diagnostic replay does not certify receipt settlement".into()],
            &std::sync::atomic::AtomicBool::new(false))?;
        println!("{}", json!({"replay":run.id,"unique_files":run.total_files(),
            "receipt_segments":run.files.len(),"diff_receipts":input.diffs.len(),
            "patch_receipts":input.patch_only_files.len(),"batches":input.batches.len(),
            "diff_characters":input.diffs.values().flatten().map(|p| p.characters).sum::<usize>(),
            "skipped":run.files.iter().filter(|f| f.status == ReviewFileStatus::Skipped).count(),
            "skip_reasons":run.files.iter().filter_map(|f| f.skip_reason.as_ref())
                .collect::<std::collections::BTreeSet<_>>(),"storage":storage,
            "historical_paths":run.files.iter().map(|f| &f.path).collect::<std::collections::BTreeSet<_>>()
                .into_iter().filter_map(|path| { let count = run.files.iter().filter(|f| &f.path == path).count();
                    (count > 1).then(|| json!({"path":path,"segments":count})) }).collect::<Vec<_>>() }));
    }
    let mut rows = connection.query(
        "SELECT payload FROM mutation_evidence WHERE agent_id=?1 ORDER BY sequence DESC LIMIT 8",
        [parent.as_str()],
    ).await?;
    while let Some(row) = rows.next().await? {
        let evidence: Value = serde_json::from_str(&row.get::<String>(0)?)?;
        println!("{}", json!({"evidence":evidence["kind"], "path":evidence["key"]["path"],
            "before_bytes":evidence["before"].as_str().map(str::len),
            "after_bytes":evidence["after"].as_str().map(str::len),
            "patch_hunks":evidence["patch"]["hunks"].as_array().map(Vec::len),
            "patch_path":evidence["patch"]["path"],
            "patch_keys":evidence["patch"].as_object().map(|p| p.keys().collect::<Vec<_>>())}));
    }
    Ok(())
}
