use super::*;
use crate::agents::{
    AgentAccessMode, AgentEffort, AgentKind, AgentModel, AgentOrigin, AgentRuntimeKind, AgentStatus,
};
use crate::git::read::fixtures::{commit_all, repo_with_commit, workdir};
use crate::git::{DiffHunk, DiffLine};
use crate::task_tracker::{IssueTrackerProvider, TaskTrackerConnection};
use crate::{DbProvider, GitWorkflowCompletionPolicy, GitWorkflowRunState};
use std::fs;

fn sample_project() -> Project {
    let mut project = Project::from_path(PathBuf::from("/tmp/choro-project"));
    project
        .presets
        .push(ScriptPreset::new("test", "cargo test"));
    project
        .db_connections
        .push(DbConnection::new("local", "mongodb://localhost:27017"));
    project
}

#[test]
fn studio_conversation_parent_and_artifacts_survive_ordinary_agent_snapshots() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let project = sample_project();
    let mut config = AppConfig::default();
    config.projects.push(project.clone());
    store.save_workspace_config(&config).unwrap();
    let ordinary = sample_agent(&project);
    store.save_agents(std::slice::from_ref(&ordinary)).unwrap();
    let mut assistant = sample_agent(&project);
    assistant.hidden_doc_assistant = true;
    assistant.title = "Studio conversation".into();
    store.ensure_assistant_chat_agent(&assistant).unwrap();
    store.replace_chat_file_ledger(assistant.id, 7, &[]).unwrap();
    store.upsert_timeline_event(assistant.id, "proposed_plan", Some("saved-plan".into()),
        "{\"revision\":7}", 1).unwrap();
    store.append_chat_message(assistant.id, "user", "Keep the complete Studio history", 1, None).unwrap();

    // A queued ordinary snapshot lacks this independently owned conversation.
    store.save_agents(std::slice::from_ref(&ordinary)).unwrap();
    // Reopening must not reset metadata that a newer save already captured.
    let mut stale = assistant.clone();
    stale.title = "Stale title".into();
    store.ensure_assistant_chat_agent(&stale).unwrap();
    let second = AgentRecord { id: Uuid::new_v4(), ..assistant.clone() };
    store.ensure_assistant_chat_agent(&second).unwrap();
    store.replace_chat_file_ledger(second.id, 1, &[]).unwrap();
    drop(store);

    let reopened = LocalStore::open(dir.path().to_path_buf()).unwrap();
    assert_eq!(reopened.load_agents().unwrap().iter().find(|a| a.id == assistant.id).unwrap().title,
        "Studio conversation");
    assert_eq!(reopened.load_chat_file_ledger(assistant.id).unwrap().unwrap().revision, 7);
    assert_eq!(reopened.load_chat_file_ledger(second.id).unwrap().unwrap().revision, 1);
    assert_eq!(reopened.load_chat_messages(assistant.id).unwrap()[0].text,
        "Keep the complete Studio history");
    assert!(reopened.load_latest_timeline_event(assistant.id, "proposed_plan").unwrap().is_some());
    assert!(reopened.load_agents().unwrap().iter().any(|a| a.id == ordinary.id));

    // Project removal must still cascade assistants; opening a stale record
    // must never resurrect a removed project.
    config.projects.clear();
    reopened.save_workspace_config(&config).unwrap();
    assert!(reopened.ensure_assistant_chat_agent(&assistant).unwrap_err().to_string().contains("project no longer exists"));
    assert!(reopened.load_chat_file_ledger(assistant.id).unwrap().is_none());
}

#[test]
fn stale_agents_from_removed_projects_do_not_block_new_chats() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let removed_project = sample_project();
    let active_project = sample_project();
    let removed_agent = sample_agent(&removed_project);
    let existing_agent = sample_agent(&active_project);
    let new_agent = sample_agent(&active_project);
    let mut config = AppConfig::default();
    config.projects = vec![removed_project, active_project.clone()];
    store.save_workspace_config(&config).unwrap();
    store.save_agents(&[removed_agent.clone(), existing_agent.clone()]).unwrap();
    store.append_chat_message(existing_agent.id, "user", "Keep existing messages", 1, None).unwrap();

    // Removing a workspace cascades its database agents, but the desktop may
    // still hold those old records in memory and in its JSON fallback.
    config.projects = vec![active_project];
    store.save_workspace_config(&config).unwrap();
    store.save_agents(&[removed_agent.clone(), existing_agent.clone(), new_agent.clone()]).unwrap();
    store.append_chat_message(new_agent.id, "user", "Start a new chat", 2, None).unwrap();
    let agents = store.load_agents().unwrap();
    assert_eq!(agents.len(), 2);
    assert!(!agents.iter().any(|agent| agent.id == removed_agent.id));
    assert_eq!(store.load_chat_messages(existing_agent.id).unwrap()[0].text, "Keep existing messages");
    assert_eq!(store.load_chat_messages(new_agent.id).unwrap().len(), 1);
}

#[test]
fn queued_project_script_survives_a_stale_workspace_save() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let project = Project::from_path(PathBuf::from("/tmp/choro-script-project"));
    let mut stale_config = AppConfig::default();
    stale_config.projects.push(project.clone());
    store.save_workspace_config(&stale_config).unwrap();

    let (_, created) = store
        .create_project_script_preset(project.id, "Preview", "npm run dev")
        .unwrap();
    assert!(created);
    store.save_workspace_config(&stale_config).unwrap();

    let loaded = store.load_workspace_config(AppConfig::default()).unwrap();
    assert_eq!(loaded.projects[0].presets.len(), 1);
    assert_eq!(loaded.projects[0].presets[0].name, "Preview");
    assert_eq!(loaded.projects[0].presets[0].command, "npm run dev");
}

#[test]
fn concurrent_project_script_creates_resolve_to_one_name() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let project = Project::from_path(PathBuf::from("/tmp/choro-concurrent-script-project"));
    let project_id = project.id;
    let mut config = AppConfig::default();
    config.projects.push(project.clone());
    store.save_workspace_config(&config).unwrap();

    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let create = || {
        let store = store.clone();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            barrier.wait();
            store
                .create_project_script_preset(project_id, "Preview", "npm run dev")
                .unwrap()
        })
    };
    let first = create();
    let second = create();
    barrier.wait();
    let first = first.join().unwrap();
    let second = second.join().unwrap();

    assert_ne!(first.1, second.1);
    assert_eq!(first.0.id, second.0.id);
    let loaded = store.load_workspace_config(AppConfig::default()).unwrap();
    assert_eq!(loaded.projects[0].presets.len(), 1);
    let error = store
        .create_project_script_preset(project_id, "preview", "npm start")
        .unwrap_err();
    assert!(error.to_string().contains("different command"));
}

#[test]
fn chat_file_ledger_round_trips_exact_and_observed_entries() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let project = sample_project();
    let agent = sample_agent(&project);
    let mut config = AppConfig::default();
    config.projects = vec![project];
    store.save_workspace_config(&config).unwrap();
    store.save_agents(std::slice::from_ref(&agent)).unwrap();
    let agent_id = agent.id;
    let entries = vec![
        StoredChatFileLedgerEntry {
            segments_json: "[]".into(),
            counts_unavailable: true,
            agent_id,
            path: PathBuf::from("src/exact.rs"),
            observed: false,
            additions: 4,
            deletions: 1,
            baseline_hash: Some("before".into()),
            result_hash: Some("after".into()),
            baseline_content: Some("before\n".into()),
            result_content: Some("after\n".into()),
            updated_at: 10,
        },
        StoredChatFileLedgerEntry {
            segments_json: "[]".into(),
            counts_unavailable: false,
            agent_id,
            path: PathBuf::from("generated.css"),
            observed: true,
            additions: 8,
            deletions: 0,
            baseline_hash: None,
            result_hash: None,
            baseline_content: None,
            result_content: None,
            updated_at: 10,
        },
    ];

    store
        .replace_chat_file_ledger(agent_id, 1, &entries)
        .unwrap();
    let mut expected = entries;
    expected.sort_by(|left, right| left.path.cmp(&right.path));
    let ledger = store.load_chat_file_ledger(agent_id).unwrap().unwrap();
    assert_eq!(ledger.revision, 1);
    assert_eq!(ledger.projection_version, 1);
    assert_eq!(ledger.entries, expected);
}

#[test]
fn chat_file_ledger_replacement_removes_reverted_paths() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let project = sample_project();
    let agent = sample_agent(&project);
    let mut config = AppConfig::default();
    config.projects = vec![project];
    store.save_workspace_config(&config).unwrap();
    store.save_agents(std::slice::from_ref(&agent)).unwrap();
    let agent_id = agent.id;
    store
        .replace_chat_file_ledger(
            agent_id,
            1,
            &[StoredChatFileLedgerEntry {
            segments_json: "[]".into(),
                counts_unavailable: false,
                agent_id,
                path: PathBuf::from("src/reverted.rs"),
                observed: false,
                additions: 1,
                deletions: 0,
                baseline_hash: Some("base".into()),
                result_hash: Some("changed".into()),
                baseline_content: Some("base\n".into()),
                result_content: Some("changed\n".into()),
                updated_at: 10,
            }],
        )
        .unwrap();
    store.replace_chat_file_ledger(agent_id, 2, &[]).unwrap();
    assert!(store
        .load_chat_file_ledger(agent_id)
        .unwrap()
        .unwrap()
        .entries
        .is_empty());
}

#[test]
fn stale_chat_file_ledger_write_cannot_overwrite_a_newer_revision() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let project = sample_project();
    let agent = sample_agent(&project);
    let mut config = AppConfig::default();
    config.projects = vec![project];
    store.save_workspace_config(&config).unwrap();
    store.save_agents(std::slice::from_ref(&agent)).unwrap();

    let entry = |path: &str| StoredChatFileLedgerEntry {
            segments_json: "[]".into(),
        counts_unavailable: false,
        agent_id: agent.id,
        path: PathBuf::from(path),
        observed: false,
        additions: 1,
        deletions: 0,
        baseline_hash: None,
        result_hash: None,
        baseline_content: None,
        result_content: None,
        updated_at: 10,
    };
    store
        .replace_chat_file_ledger(agent.id, 2, &[entry("newer.rs")])
        .unwrap();
    store
        .replace_chat_file_ledger(agent.id, 1, &[entry("stale.rs")])
        .unwrap();

    let ledger = store.load_chat_file_ledger(agent.id).unwrap().unwrap();
    assert_eq!(ledger.revision, 2);
    assert_eq!(ledger.entries[0].path, PathBuf::from("newer.rs"));
}

#[test]
fn concurrent_chat_file_ledger_writers_keep_the_newest_revision() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let project = sample_project();
    let agent = sample_agent(&project);
    let mut config = AppConfig::default();
    config.projects = vec![project];
    store.save_workspace_config(&config).unwrap();
    store.save_agents(std::slice::from_ref(&agent)).unwrap();

    let agent_id = agent.id;
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let writer = |revision, path: &'static str| {
        let store = store.clone();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            let entry = StoredChatFileLedgerEntry {
            segments_json: "[]".into(),
                counts_unavailable: false,
                agent_id,
                path: PathBuf::from(path),
                observed: false,
                additions: 1,
                deletions: 0,
                baseline_hash: None,
                result_hash: None,
                baseline_content: None,
                result_content: None,
                updated_at: revision,
            };
            barrier.wait();
            store
                .replace_chat_file_ledger(agent_id, revision, &[entry])
                .unwrap();
        })
    };
    let stale = writer(1, "stale.rs");
    let newest = writer(2, "newest.rs");
    barrier.wait();
    stale.join().unwrap();
    newest.join().unwrap();

    let ledger = store.load_chat_file_ledger(agent_id).unwrap().unwrap();
    assert_eq!(ledger.revision, 2);
    assert_eq!(ledger.entries[0].path, PathBuf::from("newest.rs"));
}

#[test]
fn changed_file_receipt_and_ledger_roll_back_together() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let project = sample_project();
    let agent = sample_agent(&project);
    let mut config = AppConfig::default();
    config.projects = vec![project];
    store.save_workspace_config(&config).unwrap();
    store.save_agents(std::slice::from_ref(&agent)).unwrap();

    let invalid_entry = StoredChatFileLedgerEntry {
            segments_json: "[]".into(),
        counts_unavailable: false,
        agent_id: agent.id,
        path: PathBuf::from("src/atomic.rs"),
        observed: false,
        additions: 1,
        deletions: 0,
        baseline_hash: None,
        result_hash: None,
        baseline_content: None,
        result_content: None,
        // Forces the ledger half of the transaction to fail after the receipt
        // has been inserted.
        updated_at: u64::MAX,
    };
    assert!(store
        .persist_timeline_event_and_chat_file_ledger(
            agent.id,
            "changed_files",
            Some("changed_files:turn:atomic".into()),
            r#"{"type":"changed_files","files":[]}"#,
            10,
            1,
            &[invalid_entry],
        )
        .is_err());

    assert!(store.load_timeline_events(agent.id).unwrap().is_empty());
    assert!(store.load_chat_file_ledger(agent.id).unwrap().is_none());
}

#[test]
fn changed_file_receipt_and_ledger_commit_together() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let project = sample_project();
    let agent = sample_agent(&project);
    let mut config = AppConfig::default();
    config.projects = vec![project];
    store.save_workspace_config(&config).unwrap();
    store.save_agents(std::slice::from_ref(&agent)).unwrap();

    let entry = StoredChatFileLedgerEntry {
            segments_json: "[]".into(),
        counts_unavailable: false,
        agent_id: agent.id,
        path: PathBuf::from("src/atomic.rs"),
        observed: false,
        additions: 1,
        deletions: 0,
        baseline_hash: None,
        result_hash: None,
        baseline_content: None,
        result_content: None,
        updated_at: 10,
    };
    store
        .persist_timeline_event_and_chat_file_ledger(
            agent.id,
            "changed_files",
            Some("changed_files:turn:atomic".into()),
            r#"{"type":"changed_files","files":[]}"#,
            10,
            1,
            std::slice::from_ref(&entry),
        )
        .unwrap();

    assert_eq!(store.load_timeline_events(agent.id).unwrap().len(), 1);
    assert_eq!(
        store
            .load_chat_file_ledger(agent.id)
            .unwrap()
            .unwrap()
            .entries,
        vec![entry]
    );
}

#[test]
fn latest_plan_and_file_receipts_are_available_outside_the_loaded_chat_page() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let project = sample_project();
    let agent = sample_agent(&project);
    let other = sample_agent(&project);
    let mut config = AppConfig::default();
    config.projects = vec![project];
    store.save_workspace_config(&config).unwrap();
    store.save_agents(&[agent.clone(), other.clone()]).unwrap();
    store.upsert_timeline_event(agent.id, "changed_files", Some("files:first".into()), "files", 1).unwrap();
    store.upsert_timeline_event(agent.id, "proposed_plan", Some("plan:first".into()), "old plan", 2).unwrap();
    store.upsert_timeline_event(agent.id, "proposed_plan", Some("plan:latest".into()), "latest plan", 3).unwrap();
    for i in 0..205 {
        store.upsert_timeline_event(agent.id, "message", Some(format!("message:{i}")), "message", 4 + i).unwrap();
    }
    // Updating an older card must not promote it above a newer plan.
    store.upsert_timeline_event(agent.id, "proposed_plan", Some("plan:first".into()), "expanded old plan", 300).unwrap();
    store.upsert_timeline_event(agent.id, "proposed_plan", Some("plan:latest".into()), "revised latest plan", 301).unwrap();
    store.upsert_timeline_event(other.id, "proposed_plan", Some("other".into()), "other agent plan", 302).unwrap();
    drop(store);
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let page = store.load_timeline_events_page(agent.id, None, 200).unwrap();
    assert!(page.has_more);
    assert!(page.events.iter().all(|event| event.kind == "message"));
    assert_eq!(store.load_latest_timeline_event(agent.id, "proposed_plan").unwrap().unwrap().payload_json, "revised latest plan");
    let receipts = store.load_timeline_events_by_kind(agent.id, "changed_files").unwrap();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].payload_json, "files");
}

#[test]
fn newer_plan_survives_late_older_persistence_and_old_plan_updates() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let project = sample_project();
    let agent = sample_agent(&project);
    let mut config = AppConfig::default();
    config.projects = vec![project];
    store.save_workspace_config(&config).unwrap();
    store.save_agents(std::slice::from_ref(&agent)).unwrap();
    let write = |key: &str, revision, markdown: &str, implemented_at| {
        store.upsert_timeline_event(agent.id, "proposed_plan", Some(key.into()),
            serde_json::json!({"revision":revision,"markdown":markdown,"implemented_at":implemented_at}).to_string(), 10).unwrap();
    };
    write("new-plan", 2, "Latest contents", None);
    write("old-plan", 1, "Late old plan", None);
    write("new-plan", 1, "Late old version", None);
    write("new-plan", 2, "Latest contents", Some(20));
    write("new-plan", 2, "Latest contents", None);
    drop(store);
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let latest = store.load_latest_timeline_event(agent.id, "proposed_plan").unwrap().unwrap();
    let value: serde_json::Value = serde_json::from_str(&latest.payload_json).unwrap();
    assert_eq!(latest.event_key.as_deref(), Some("new-plan"));
    assert_eq!(value["markdown"], "Latest contents");
    assert_eq!(value["implemented_at"], 20);
}

#[test]
fn migrates_v29_to_chat_file_ledger_schema() {
    let dir = tempfile::tempdir().unwrap();
    {
        let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
        store
            .rt
            .block_on(async {
                let conn = store.connect().await?;
                conn.execute("DROP TABLE chat_file_ledger", ()).await?;
                conn.execute("DROP TABLE chat_file_ledgers", ()).await?;
                conn.execute("DELETE FROM schema_migrations WHERE version >= 30", ())
                    .await?;
                assert_eq!(schema_version(&conn).await?, 29);
                Ok::<_, anyhow::Error>(())
            })
            .unwrap();
    }

    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    store
        .rt
        .block_on(async {
            let conn = store.connect().await?;
            assert_eq!(schema_version(&conn).await?, STORE_SCHEMA_VERSION);
            assert!(table_exists(&conn, "chat_file_ledgers").await?);
            assert!(table_exists(&conn, "chat_file_ledger").await?);
            Ok::<_, anyhow::Error>(())
        })
        .unwrap();
}

#[test]
fn migrating_v39_preserves_legacy_file_projections_until_they_are_recovered() {
    let dir = tempfile::tempdir().unwrap();
    let project = sample_project();
    let agent = sample_agent(&project);
    {
        let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
        let mut config = AppConfig::default();
        config.projects = vec![project];
        store.save_workspace_config(&config).unwrap();
        store.save_agents(std::slice::from_ref(&agent)).unwrap();
        store.replace_chat_file_ledger(agent.id, 7, &[]).unwrap();
        store.rt.block_on(async {
            let conn = store.connect().await?;
            // Simulate the metadata of an existing pre-versioned projection.
            conn.execute("UPDATE chat_file_ledgers SET projection_version = 0 WHERE agent_id = ?1", [agent.id.to_string()]).await?;
            conn.execute("DELETE FROM schema_migrations WHERE version > 39", ()).await?;
            Ok::<_, anyhow::Error>(())
        }).unwrap();
    }
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let legacy = store.load_chat_file_ledger(agent.id).unwrap().unwrap();
    assert_eq!(legacy.revision, 7);
    assert_eq!(legacy.projection_version, 0);
    assert!(legacy.entries.is_empty());
    let archive = dir.path().join("legacy-export.zip");
    store.export_workspace(&archive).unwrap();
    let destination = tempfile::tempdir().unwrap();
    let imported = LocalStore::open(destination.path().to_path_buf()).unwrap();
    imported.import_workspace_replace(&archive).unwrap();
    assert_eq!(imported.load_chat_file_ledger(agent.id).unwrap().unwrap(), legacy);
    store.replace_chat_file_ledger(agent.id, 7, &[]).unwrap();
    drop(store);
    let reopened = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let saved = reopened.load_chat_file_ledger(agent.id).unwrap().unwrap();
    assert_eq!(saved.projection_version, 1);
    assert_eq!(saved.revision, 7);
    assert!(saved.entries.is_empty());
}

#[test]
fn migrates_v25_to_git_workflow_schema() {
    let dir = tempfile::tempdir().unwrap();
    {
        let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
        store
            .rt
            .block_on(async {
                let conn = store.connect().await?;
                conn.execute("DROP TABLE project_git_workflow_runs", ())
                    .await?;
                conn.execute("DROP TABLE project_git_workflows", ()).await?;
                conn.execute("DELETE FROM schema_migrations WHERE version >= 26", ())
                    .await?;
                assert_eq!(schema_version(&conn).await?, 25);
                Ok::<_, anyhow::Error>(())
            })
            .unwrap();
    }

    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    store
        .rt
        .block_on(async {
            let conn = store.connect().await?;
            assert_eq!(schema_version(&conn).await?, STORE_SCHEMA_VERSION);
            assert!(table_exists(&conn, "project_git_workflows").await?);
            assert!(table_exists(&conn, "project_git_workflow_runs").await?);
            assert!(column_exists(&conn, "project_git_workflows", "source").await?);
            assert!(column_exists(&conn, "project_git_workflow_runs", "destination").await?);
            Ok::<_, anyhow::Error>(())
        })
        .unwrap();
}

#[test]
fn empty_v25_workspace_migrates_with_empty_workflow_collections() {
    let dir = tempfile::tempdir().unwrap();
    {
        let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
        store
            .rt
            .block_on(async {
                let conn = store.connect().await?;
                conn.execute("DELETE FROM projects", ()).await?;
                conn.execute("DROP TABLE project_git_workflow_runs", ())
                    .await?;
                conn.execute("DROP TABLE project_git_workflows", ()).await?;
                conn.execute("DELETE FROM schema_migrations WHERE version >= 26", ())
                    .await?;
                assert_eq!(schema_version(&conn).await?, 25);
                Ok::<_, anyhow::Error>(())
            })
            .unwrap();
    }

    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let loaded = store.load_workspace_config(AppConfig::default()).unwrap();
    assert!(loaded.projects.is_empty());
    store
        .rt
        .block_on(async {
            let conn = store.connect().await?;
            assert_eq!(schema_version(&conn).await?, STORE_SCHEMA_VERSION);
            let mut workflow_rows = conn
                .query("SELECT COUNT(*) FROM project_git_workflows", ())
                .await?;
            let workflows: i64 = workflow_rows.next().await?.unwrap().get(0)?;
            let mut run_rows = conn
                .query("SELECT COUNT(*) FROM project_git_workflow_runs", ())
                .await?;
            let runs: i64 = run_rows.next().await?.unwrap().get(0)?;
            assert_eq!(workflows, 0);
            assert_eq!(runs, 0);
            Ok::<_, anyhow::Error>(())
        })
        .unwrap();
}

#[test]
fn git_workflows_and_runs_round_trip_and_delete_keeps_history() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let mut config = AppConfig::default();
    let mut project = sample_project();
    let workflow = GitWorkflow::new(
        PathBuf::from("apps/api"),
        "Promote staging",
        "staging",
        "production",
        GitWorkflowCompletionPolicy::AutoMergeWhenReady,
    )
    .unwrap();
    let workflow_id = workflow.id;
    project.git_workflows.push(workflow.clone());
    project.git_workflow_runs.push(GitWorkflowRun {
        id: Uuid::new_v4(),
        workflow_id: Some(workflow_id),
        repository_path: PathBuf::from("apps/api"),
        source_branch: "staging".into(),
        destination_branch: "production".into(),
        pull_request_number: Some(42),
        expected_head_sha: Some("0123456789abcdef".into()),
        state: GitWorkflowRunState::AutoMergeEnabled,
        error: None,
        started_at: 100,
        updated_at: 200,
    });
    config.projects.push(project);
    store.save_workspace_config(&config).unwrap();

    let loaded = store.load_workspace_config(AppConfig::default()).unwrap();
    assert_eq!(loaded.projects[0].git_workflows, vec![workflow]);
    assert_eq!(loaded.projects[0].git_workflow_runs.len(), 1);
    assert_eq!(
        loaded.projects[0].git_workflow_runs[0].pull_request_number,
        Some(42)
    );

    config.projects[0].git_workflows.clear();
    store.save_workspace_config(&config).unwrap();
    let loaded = store.load_workspace_config(AppConfig::default()).unwrap();
    assert!(loaded.projects[0].git_workflows.is_empty());
    assert_eq!(loaded.projects[0].git_workflow_runs.len(), 1);
    assert_eq!(loaded.projects[0].git_workflow_runs[0].workflow_id, None);
}

#[test]
fn git_workflow_persistence_prunes_old_terminal_runs_only() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let mut config = AppConfig::default();
    let mut project = sample_project();
    for updated_at in 0..35 {
        project.git_workflow_runs.push(GitWorkflowRun {
            id: Uuid::new_v4(),
            workflow_id: None,
            repository_path: PathBuf::from("."),
            source_branch: "staging".into(),
            destination_branch: "production".into(),
            pull_request_number: None,
            expected_head_sha: None,
            state: GitWorkflowRunState::Merged,
            error: None,
            started_at: updated_at,
            updated_at,
        });
    }
    project.git_workflow_runs.push(GitWorkflowRun {
        id: Uuid::new_v4(),
        workflow_id: None,
        repository_path: PathBuf::from("."),
        source_branch: "release".into(),
        destination_branch: "main".into(),
        pull_request_number: Some(99),
        expected_head_sha: None,
        state: GitWorkflowRunState::WaitingForRequirements,
        error: None,
        started_at: 1,
        updated_at: 1,
    });
    config.projects.push(project);
    store.save_workspace_config(&config).unwrap();

    let loaded = store.load_workspace_config(AppConfig::default()).unwrap();
    assert_eq!(loaded.projects[0].git_workflow_runs.len(), 31);
    assert_eq!(
        loaded.projects[0]
            .git_workflow_runs
            .iter()
            .filter(|run| run.state == GitWorkflowRunState::Merged)
            .count(),
        30
    );
    assert!(loaded.projects[0]
        .git_workflow_runs
        .iter()
        .any(|run| run.state == GitWorkflowRunState::WaitingForRequirements));
}

fn sample_agent(project: &Project) -> AgentRecord {
    let mut agent = AgentRecord::new(
        project.id,
        project.path.clone(),
        "Implement storage",
        "Move storage to Turso",
        AgentKind::Codex,
        AgentModel::CodexDefault,
        AgentEffort::Medium,
        AgentAccessMode::FullAccess,
    );
    agent.runtime = AgentRuntimeKind::Chat;
    agent.status = AgentStatus::InProgress;
    agent.linked_docs.push(PathBuf::from("docs/storage.md"));
    agent.changed_files.push(AgentChangedFile {
        path: PathBuf::from("src/lib.rs"),
        additions: 5,
        deletions: 1,
    });
    agent
}

#[test]
fn brain_summary_search_recall_and_message_bus_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let project = sample_project();
    let mut config = AppConfig::default();
    config.projects.push(project.clone());
    store.save_workspace_config(&config).unwrap();

    let mut requester = sample_agent(&project);
    requester.title = "Current agent".into();
    let mut target = sample_agent(&project);
    target.title = "Storage migration".into();
    target.id = Uuid::new_v4();
    store
        .save_agents(&[requester.clone(), target.clone()])
        .unwrap();

    store
        .append_chat_message(target.id, "user", "Move the cache into SQLite", 10, None)
        .unwrap();
    store
        .append_chat_message(
            target.id,
            "assistant",
            "Implemented the storage layer",
            11,
            None,
        )
        .unwrap();
    let summary = store
        .save_agent_summary(
            target.id,
            "Migrated cache storage to SQLite. Touched src/storage.rs and verified restart behavior.",
            Some("Migrated cache storage to SQLite and verified restart behavior."),
            false,
        )
        .unwrap();
    assert_eq!(summary.last_summarized_sequence, 1);
    assert_eq!(
        summary.outcome_text.as_deref(),
        Some("Migrated cache storage to SQLite and verified restart behavior.")
    );

    let results = store
        .search_agents(requester.id, "SQLite storage", false, 10)
        .unwrap();
    assert!(results.iter().any(|result| result.agent_id == target.id));
    assert!(store
        .search_agents_for_files(requester.id, "SQLite storage", false, None, 10)
        .unwrap()
        .iter()
        .any(|result| result.agent_id == target.id));
    assert!(store
        .search_agents_for_files(
            requester.id,
            "SQLite storage",
            false,
            Some(&["src/not-touched.rs".to_string()]),
            10,
        )
        .unwrap()
        .is_empty());

    store
        .append_chat_message(
            target.id,
            "assistant",
            "The unique quokka-checkpoint appears only in the transcript.",
            12,
            None,
        )
        .unwrap();
    assert!(store
        .search_agents(requester.id, "quokka-checkpoint", false, 10)
        .unwrap()
        .is_empty());
    assert_eq!(
        store
            .search_agents(requester.id, "quokka-checkpoint", true, 10)
            .unwrap()[0]
            .agent_id,
        target.id
    );
    // Penpot conversations share chat_messages but have no matching agents
    // row, so they must never enter the cross-agent full-text index.
    store
        .append_chat_message(
            Uuid::new_v4(),
            "assistant",
            "penpotplatypusunique",
            13,
            None,
        )
        .unwrap();
    assert!(store
        .search_agents(requester.id, "penpotplatypusunique", true, 10)
        .unwrap()
        .is_empty());

    for index in 0..19 {
        store
            .append_chat_message(
                target.id,
                "assistant",
                &format!("checkpoint message {index}"),
                20 + index,
                None,
            )
            .unwrap();
    }
    assert!(store.agent_summary_refresh_due(target.id, 20, 0).unwrap());

    let page = store
        .recall_agent(requester.id, target.id, None, 1)
        .unwrap();
    assert_eq!(page.summary.as_ref(), Some(&summary));
    assert_eq!(page.messages.len(), 1);
    assert!(page.has_more);
    assert_eq!(page.next_before_sequence, Some(21));

    let edited = store
        .save_agent_summary(
            target.id,
            "User-verified SQLite migration.",
            Some("Verified the SQLite migration."),
            true,
        )
        .unwrap();
    assert!(edited.edited_by_user);
    assert_eq!(edited.last_summarized_sequence, 1);

    let message = store
        .send_agent_message(
            requester.id,
            target.id,
            "Check the restart edge case.",
            "agent",
            Some("handoff:restart".into()),
        )
        .unwrap();
    let duplicate = store
        .send_agent_message(
            requester.id,
            target.id,
            "Check the restart edge case.",
            "agent",
            Some("handoff:restart".into()),
        )
        .unwrap();
    assert_eq!(message.id, duplicate.id);
    assert_eq!(store.load_pending_agent_messages().unwrap().len(), 1);
    store.mark_agent_message_delivered(message.id).unwrap();
    assert!(store.load_pending_agent_messages().unwrap().is_empty());
}

#[test]
fn free_text_agent_requests_classify_questions_and_actions() {
    for question in [
        "What did you implement in authentication?",
        "Can you explain the storage decision?",
        "Tell me the current status",
    ] {
        assert_eq!(
            classify_agent_request(question),
            AgentRequestKind::Ask,
            "expected question: {question}"
        );
    }
    for task in [
        "Please implement the refresh endpoint",
        "Can you fix the restart bug?",
        "Handle this next",
    ] {
        assert_eq!(
            classify_agent_request(task),
            AgentRequestKind::Delegate,
            "expected task: {task}"
        );
    }
    assert_eq!(AgentRequestKind::Ask.toggled(), AgentRequestKind::Delegate);
    assert_eq!(AgentRequestKind::Delegate.toggled(), AgentRequestKind::Ask);
}

#[test]
fn agent_reply_returns_to_the_request_source_once() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let project = sample_project();
    let mut config = AppConfig::default();
    config.projects.push(project.clone());
    store.save_workspace_config(&config).unwrap();
    let source = sample_agent(&project);
    let mut target = sample_agent(&project);
    target.id = Uuid::new_v4();
    target.title = "Backend agent".into();
    let mut stranger = sample_agent(&project);
    stranger.id = Uuid::new_v4();
    stranger.title = "Unrelated agent".into();
    store
        .save_agents(&[source.clone(), target.clone(), stranger.clone()])
        .unwrap();

    let request = store
        .send_agent_message(
            source.id,
            target.id,
            "What authentication contract did you implement?",
            AgentRequestKind::Ask.storage_label(),
            None,
        )
        .unwrap();
    assert_eq!(
        store.load_pending_agent_messages().unwrap(),
        vec![request.clone()],
        "the request stays durable until the target returns its reply"
    );

    assert!(store
        .reply_to_agent_message(stranger.id, request.id, "Forged answer")
        .is_err());
    let reply = store
        .reply_to_agent_message(target.id, request.id, "JWT with rotating refresh tokens.")
        .unwrap();
    let duplicate = store
        .reply_to_agent_message(
            target.id,
            request.id,
            "A retry must not replace the answer.",
        )
        .unwrap();
    assert_eq!(reply.id, duplicate.id);
    assert_eq!(duplicate.text, "JWT with rotating refresh tokens.");
    assert_eq!(reply.source_agent_id, target.id);
    assert_eq!(reply.target_agent_id, source.id);
    assert_eq!(reply.source_title, "Backend agent");
    assert_eq!(reply.kind, "reply");
    assert_eq!(
        reply.event_key.as_deref(),
        Some(format!("reply:{}", request.id).as_str())
    );

    let pending = store.load_pending_agent_messages().unwrap();
    assert_eq!(
        pending,
        vec![reply],
        "saving the reply acknowledges the original request"
    );
}

#[test]
fn concurrent_agent_replies_resolve_to_one_durable_reply() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let project = sample_project();
    let mut config = AppConfig::default();
    config.projects.push(project.clone());
    store.save_workspace_config(&config).unwrap();
    let source = sample_agent(&project);
    let mut target = sample_agent(&project);
    target.id = Uuid::new_v4();
    target.title = "Backend agent".into();
    store
        .save_agents(&[source.clone(), target.clone()])
        .unwrap();
    let request = store
        .send_agent_message(
            source.id,
            target.id,
            "Can you check this?",
            AgentRequestKind::Ask.storage_label(),
            Some("request:concurrent-reply".to_string()),
        )
        .unwrap();

    let first_store = store.clone();
    let second_store = store.clone();
    let (first, second) = std::thread::scope(|scope| {
        let first =
            scope.spawn(|| first_store.reply_to_agent_message(target.id, request.id, "Checked."));
        let second =
            scope.spawn(|| second_store.reply_to_agent_message(target.id, request.id, "Checked."));
        (
            first.join().unwrap().unwrap(),
            second.join().unwrap().unwrap(),
        )
    });

    assert_eq!(first.id, second.id);
    assert_eq!(store.load_pending_agent_messages().unwrap(), vec![first]);
}

#[test]
fn brain_cross_agent_reads_are_available_without_a_per_agent_toggle() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let project = sample_project();
    let mut config = AppConfig::default();
    config.projects.push(project.clone());
    store.save_workspace_config(&config).unwrap();
    let requester = sample_agent(&project);
    let mut target = sample_agent(&project);
    target.id = Uuid::new_v4();
    target.title = "Storage indexing".into();
    store
        .save_agents(&[requester.clone(), target.clone()])
        .unwrap();
    store
        .save_agent_summary(
            target.id,
            "Implemented indexed storage recall.",
            None,
            false,
        )
        .unwrap();
    assert_eq!(
        store
            .load_agent_summary(target.id)
            .unwrap()
            .unwrap()
            .outcome_text,
        None
    );

    let results = store
        .search_agents(requester.id, "storage", false, 10)
        .unwrap();
    assert!(results.iter().any(|result| result.agent_id == target.id));
}

#[test]
fn migrates_v26_to_current_brain_schema_and_fts_indexes() {
    let dir = tempfile::tempdir().unwrap();
    {
        let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
        store
            .rt
            .block_on(async {
                conn_cleanup_brain_v27(&store.connect().await?).await?;
                Ok::<_, anyhow::Error>(())
            })
            .unwrap();
    }

    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    store
        .rt
        .block_on(async {
            let conn = store.connect().await?;
            assert_eq!(schema_version(&conn).await?, STORE_SCHEMA_VERSION);
            assert!(table_exists(&conn, "agent_summaries").await?);
            assert!(column_exists(&conn, "agent_summaries", "outcome_text").await?);
            assert!(table_exists(&conn, "agent_search_fts").await?);
            assert!(table_exists(&conn, "chat_messages_fts").await?);
            assert!(table_exists(&conn, "agent_messages").await?);
            Ok::<_, anyhow::Error>(())
        })
        .unwrap();
}

#[test]
fn brain_outcome_migration_does_not_backfill_existing_summaries() {
    let dir = tempfile::tempdir().unwrap();
    let project = sample_project();
    let agent = sample_agent(&project);
    {
        let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
        let mut config = AppConfig::default();
        config.projects.push(project.clone());
        store.save_workspace_config(&config).unwrap();
        store.save_agents(std::slice::from_ref(&agent)).unwrap();
        store
            .save_agent_summary(agent.id, "An older living summary.", None, false)
            .unwrap();
        store
            .rt
            .block_on(async {
                let conn = store.connect().await?;
                conn.execute("DELETE FROM schema_migrations WHERE version >= 28", ())
                    .await?;
                assert_eq!(schema_version(&conn).await?, 27);
                Ok::<_, anyhow::Error>(())
            })
            .unwrap();
    }

    let reopened = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let summary = reopened.load_agent_summary(agent.id).unwrap().unwrap();
    assert_eq!(summary.outcome_text, None);
}

async fn conn_cleanup_brain_v27(conn: &Connection) -> anyhow::Result<()> {
    conn.execute("DROP TABLE agent_messages", ()).await?;
    conn.execute("DROP TABLE agent_search_fts", ()).await?;
    conn.execute("DROP TABLE chat_messages_fts", ()).await?;
    conn.execute("DROP TABLE agent_summaries", ()).await?;
    conn.execute("DELETE FROM schema_migrations WHERE version >= 27", ())
        .await?;
    assert_eq!(schema_version(conn).await?, 26);
    Ok(())
}

fn sample_task_connection() -> TaskTrackerConnection {
    let mut connection = TaskTrackerConnection::new_jira(
        "Product Jira",
        "https://example.atlassian.net",
        "dev@example.com",
        "${JIRA_API_TOKEN}",
    );
    connection.id = Uuid::parse_str("00000000-0000-0000-0000-000000000101").unwrap();
    connection.board_id = Some(42);
    connection.board_name = Some("Engineering".to_string());
    connection.assignee_account_id = Some("712020:abc-123".to_string());
    connection.assignee_display_name = Some("Ada Lovelace".to_string());
    connection
}

fn sample_task_ref() -> TaskRef {
    TaskRef {
        provider: IssueTrackerProvider::Jira,
        site_url: "https://example.atlassian.net".to_string(),
        issue_id: "10001".to_string(),
        issue_key: "APP-123".to_string(),
        issue_url: "https://example.atlassian.net/browse/APP-123".to_string(),
        title: "Add task board".to_string(),
    }
}

fn sample_diff(path: &str) -> FileDiff {
    FileDiff {
        path: PathBuf::from(path),
        is_binary: false,
        hunks: vec![DiffHunk {
            header: "@@ -1 +1,2 @@".to_string(),
            lines: vec![
                DiffLine {
                    origin: LineOrigin::Remove,
                    old_no: Some(1),
                    new_no: None,
                    text: "old".to_string(),
                },
                DiffLine {
                    origin: LineOrigin::Add,
                    old_no: None,
                    new_no: Some(1),
                    text: "new".to_string(),
                },
                DiffLine {
                    origin: LineOrigin::Add,
                    old_no: None,
                    new_no: Some(2),
                    text: "more".to_string(),
                },
            ],
        }],
    }
}

fn write_test_png(path: &Path) {
    let mut image = image::RgbaImage::new(16, 16);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        *pixel = image::Rgba([(x * 12) as u8, (y * 12) as u8, 180, 255]);
    }
    image
        .save_with_format(path, image::ImageFormat::Png)
        .unwrap();
}

#[test]
fn migrates_schema_in_temp_root() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    assert!(store.db_path().exists());
    let agents = store.load_agents().unwrap();
    assert!(agents.is_empty());
}

#[test]
fn quick_ask_history_round_trips_and_clears_globally() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let project = sample_project();
    let session_id = Uuid::new_v4();

    let project_exchange = store
        .save_quick_ask_exchange(
            session_id,
            Some((project.id, &project.name)),
            "  Where is auth configured?  ",
            "  In the remote auth module.  ",
            "Codex",
            "GPT-5.6 Luna",
        )
        .unwrap();

    let history = store.load_quick_ask_exchanges().unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].id, project_exchange.id);
    assert_eq!(history[0].session_id, session_id);
    assert_eq!(history[0].project_id, Some(project.id));
    assert_eq!(
        history[0].project_name.as_deref(),
        Some(project.name.as_str())
    );
    assert_eq!(history[0].question, "Where is auth configured?");
    assert_eq!(history[0].answer, "In the remote auth module.");
    assert!(store
        .save_quick_ask_exchange(session_id, None, "   ", "answer", "Codex", "GPT-5.6 Luna",)
        .is_err());

    let general_exchange = store
        .save_quick_ask_exchange(
            session_id,
            None,
            "General question",
            "General answer",
            "Codex",
            "GPT-5.6 Luna",
        )
        .unwrap();
    let history = store.load_quick_ask_exchanges().unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].id, general_exchange.id);
    assert_eq!(history[1].id, project_exchange.id);
    assert_eq!(history[1].project_id, Some(project.id));
    assert_eq!(
        history[1].project_name.as_deref(),
        Some(project.name.as_str())
    );

    store.clear_quick_ask_exchanges().unwrap();
    assert!(store.load_quick_ask_exchanges().unwrap().is_empty());
}

#[test]
fn migrates_v34_quick_ask_history_from_v33_db() {
    let dir = tempfile::tempdir().unwrap();
    let project = sample_project();
    let agent = sample_agent(&project);
    {
        let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
        let mut config = AppConfig::default();
        config.projects = vec![project.clone()];
        store.save_workspace_config(&config).unwrap();
        store.save_agents(&[agent.clone()]).unwrap();
        store
            .save_voice_turn(
                "project_talk",
                "assistant",
                "Voice history survives v34",
                None,
            )
            .unwrap();
        store
            .rt
            .block_on(async {
                let conn = store.connect().await?;
                conn.execute("DROP TABLE quick_ask_exchanges", ()).await?;
                conn.execute("DELETE FROM schema_migrations WHERE version >= 34", ())
                    .await?;
                assert_eq!(schema_version(&conn).await?, 33);
                Ok::<_, anyhow::Error>(())
            })
            .unwrap();
    }

    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    assert_eq!(
        store
            .load_workspace_config(AppConfig::default())
            .unwrap()
            .projects,
        vec![project]
    );
    assert_eq!(store.load_agents().unwrap(), vec![agent]);
    assert!(store
        .load_voice_turns(10)
        .unwrap()
        .iter()
        .any(|turn| turn.text == "Voice history survives v34"));
    store
        .save_quick_ask_exchange(
            Uuid::new_v4(),
            None,
            "Migrated?",
            "Yes.",
            "Claude",
            "Sonnet",
        )
        .unwrap();
    assert_eq!(store.load_quick_ask_exchanges().unwrap().len(), 1);
}

#[test]
fn voice_turns_round_trip_and_clear_without_audio() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let agent_id = Uuid::new_v4();

    let user = store
        .save_voice_turn("director", "user", "  Check the API chat  ", None)
        .unwrap();
    let assistant = store
        .save_voice_turn(
            "director",
            "assistant",
            "The API chat is still running.",
            Some(agent_id),
        )
        .unwrap();
    let project_talk = store
        .save_voice_turn(
            "project_talk",
            "assistant",
            "This is a Rust workspace.",
            None,
        )
        .unwrap();
    let command = store
        .save_voice_turn("command", "user", "send", Some(agent_id))
        .unwrap();

    let turns = store.load_voice_turns(10).unwrap();
    assert_eq!(turns.len(), 4);
    assert_eq!(turns[0].id, user.id);
    assert_eq!(turns[0].text, "Check the API chat");
    assert_eq!(turns[1].id, assistant.id);
    assert_eq!(turns[1].agent_id, Some(agent_id));
    assert_eq!(turns[2].id, project_talk.id);
    assert_eq!(turns[2].mode, "project_talk");
    assert_eq!(turns[3].id, command.id);
    assert_eq!(turns[3].mode, "command");
    assert!(store
        .save_voice_turn("unknown", "user", "No", None)
        .is_err());
    assert!(store
        .save_voice_turn("dictation", "user", "   ", None)
        .is_err());

    store.clear_voice_turns().unwrap();
    assert!(store.load_voice_turns(10).unwrap().is_empty());
}

#[test]
fn current_schema_repairs_missing_voice_table() {
    let dir = tempfile::tempdir().unwrap();
    {
        let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
        store
            .rt
            .block_on(async {
                let conn = store.connect().await?;
                conn.execute("DROP TABLE voice_turns", ()).await?;
                assert_eq!(schema_version(&conn).await?, STORE_SCHEMA_VERSION);
                Ok::<_, anyhow::Error>(())
            })
            .unwrap();
    }

    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    store
        .save_voice_turn("dictation", "user", "Restored", None)
        .unwrap();
    assert_eq!(store.load_voice_turns(10).unwrap()[0].text, "Restored");
}

#[test]
fn migrates_v23_voice_turns_to_the_expanded_mode_set() {
    let dir = tempfile::tempdir().unwrap();
    let legacy_id = Uuid::new_v4();
    {
        let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
        store
            .rt
            .block_on(async {
                let conn = store.connect().await?;
                conn.execute("DROP TABLE voice_turns", ()).await?;
                for statement in SCHEMA_V23 {
                    conn.execute(statement, ()).await?;
                }
                conn.execute(
                    "INSERT INTO voice_turns (id, mode, role, text, agent_id, created_at)
                     VALUES (?1, 'director', 'user', 'Legacy turn', NULL, ?2)",
                    params![legacy_id.to_string(), u64_to_i64(unix_now())?],
                )
                .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version >= 24", ())
                    .await?;
                assert_eq!(schema_version(&conn).await?, 23);
                Ok::<_, anyhow::Error>(())
            })
            .unwrap();
    }

    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    store
        .save_voice_turn("project_talk", "assistant", "Migrated", None)
        .unwrap();
    store
        .save_voice_turn("command", "user", "send", None)
        .unwrap();
    let turns = store.load_voice_turns(10).unwrap();
    assert_eq!(turns.len(), 3);
    assert_eq!(turns[0].id, legacy_id);
    assert_eq!(turns[1].mode, "project_talk");
    assert_eq!(turns[2].mode, "command");
}

#[test]
fn solo_lane_fields_round_trip_through_the_store() {
    let dir = tempfile::tempdir().unwrap();
    let project = sample_project();
    let mut solo = sample_agent(&project);
    solo.repository_path = Some(project.path.join("apps/desktop"));
    solo.lane_path = Some(PathBuf::from("/tmp/choro-data/lanes/p/a"));
    solo.solo_branch = Some("solo/implement-storage".to_string());
    solo.solo_base_branch = Some("main".to_string());
    solo.solo_rejoined_branch = Some("develop".to_string());
    solo.lane_profile = Some(LaneProfile::Full);
    let mut legacy = sample_agent(&project);
    legacy.id = Uuid::new_v4();

    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let mut config = AppConfig::default();
    config.projects = vec![project.clone()];
    store.save_workspace_config(&config).unwrap();
    store.save_agents(&[solo.clone(), legacy.clone()]).unwrap();

    let loaded = store.load_agents().unwrap();
    let loaded_solo = loaded.iter().find(|agent| agent.id == solo.id).unwrap();
    assert_eq!(loaded_solo.lane_path, solo.lane_path);
    assert_eq!(loaded_solo.repository_path, solo.repository_path);
    assert_eq!(loaded_solo.solo_branch, solo.solo_branch);
    assert_eq!(loaded_solo.solo_base_branch, solo.solo_base_branch);
    assert_eq!(loaded_solo.solo_rejoined_branch, solo.solo_rejoined_branch);
    assert_eq!(loaded_solo.lane_profile, Some(LaneProfile::Full));
    assert!(loaded_solo.is_solo());
    assert!(!loaded_solo.is_active_solo());

    let loaded_legacy = loaded.iter().find(|agent| agent.id == legacy.id).unwrap();
    assert_eq!(loaded_legacy.lane_path, None);
    assert_eq!(loaded_legacy.solo_branch, None);
    assert_eq!(loaded_legacy.lane_profile, None);
    assert!(!loaded_legacy.is_solo());
}

#[test]
fn verification_completion_round_trips_through_the_store() {
    let dir = tempfile::tempdir().unwrap();
    let project = sample_project();
    let mut agent = sample_agent(&project);
    agent.verification_completed_at = Some(42);
    agent.verification_closed = true;
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let mut config = AppConfig::default();
    config.projects = vec![project];
    store.save_workspace_config(&config).unwrap();
    store.save_agents(&[agent.clone()]).unwrap();

    assert_eq!(store.load_agents().unwrap(), vec![agent]);
}

#[test]
fn pocketcomet_origin_round_trips_through_the_store() {
    let dir = tempfile::tempdir().unwrap();
    let project = sample_project();
    let mut agent = sample_agent(&project);
    agent.origin = Some(AgentOrigin::PocketComet {
        workspace_id: "workspace-1".into(),
        project_id: "project-1".into(),
        task_id: "task-1".into(),
        task_title: "Implement the integration".into(),
    });
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let mut config = AppConfig::default();
    config.projects = vec![project];
    store.save_workspace_config(&config).unwrap();
    store.save_agents(&[agent.clone()]).unwrap();

    assert_eq!(store.load_agents().unwrap(), vec![agent]);
}

#[test]
fn pocketcomet_chat_origin_round_trips_through_the_store() {
    let dir = tempfile::tempdir().unwrap();
    let project = sample_project();
    let mut agent = sample_agent(&project);
    agent.origin = Some(AgentOrigin::PocketCometChat {
        workspace_id: "workspace-1".into(),
        workspace_name: "Acme".into(),
        project_id: "project-1".into(),
        project_name: "Launch".into(),
        teammate_id: "agent-1".into(),
        teammate_name: "Choro".into(),
        conversation_id: "conversation-1".into(),
        conversation_name: "#product".into(),
        thread_id: "thread-1".into(),
        thread_title: "Should we ship this?".into(),
    });
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let mut config = AppConfig::default();
    config.projects = vec![project];
    store.save_workspace_config(&config).unwrap();
    store.save_agents(&[agent.clone()]).unwrap();

    assert_eq!(store.load_agents().unwrap(), vec![agent]);
}

#[test]
fn verification_dismissal_round_trips_as_a_hard_closed_flag() {
    let dir = tempfile::tempdir().unwrap();
    let project = sample_project();
    let mut agent = sample_agent(&project);
    agent.verification_closed = true;
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let mut config = AppConfig::default();
    config.projects = vec![project];
    store.save_workspace_config(&config).unwrap();
    store.save_agents(&[agent.clone()]).unwrap();

    let loaded = store.load_agents().unwrap();
    assert_eq!(loaded, vec![agent]);
    assert!(loaded[0].is_verification_closed());
    assert_eq!(loaded[0].verification_completed_at, None);
}

#[test]
fn memories_round_trip_scopes_and_mutations() {
    let dir = tempfile::tempdir().unwrap();
    let project = sample_project();
    let other = Project::from_path(PathBuf::from("/tmp/choro-other"));
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let mut config = AppConfig::default();
    config.projects = vec![project.clone(), other.clone()];
    store.save_workspace_config(&config).unwrap();

    let global = store
        .save_memory("global", None, "Keep answers short.", None)
        .unwrap();
    let ours = store
        .save_memory(
            "project",
            Some(project.id),
            "Themes are generated — edit tokens.",
            None,
        )
        .unwrap();
    let _theirs = store
        .save_memory("project", Some(other.id), "Different repo rule.", None)
        .unwrap();

    // Guardrails: bad scope / missing project / empty text refuse.
    assert!(store.save_memory("weird", None, "x", None).is_err());
    assert!(store.save_memory("project", None, "x", None).is_err());
    assert!(store.save_memory("global", None, "   ", None).is_err());
    assert!(store
        .save_memory("global", None, &"x".repeat(MAX_MEMORY_TEXT_CHARS + 1), None,)
        .is_err());

    // A project's session set = global + its own rows only.
    let for_project = store.load_memories_for_project(project.id).unwrap();
    let ids: Vec<Uuid> = for_project.iter().map(|memory| memory.id).collect();
    assert!(ids.contains(&global.id));
    assert!(ids.contains(&ours.id));
    assert_eq!(for_project.len(), 2);
    assert!(store.load_all_memories().unwrap().len() == 3);

    // Mutations round-trip.
    store
        .update_memory_text(ours.id, "Edit tokens only.")
        .unwrap();
    store.set_memory_pinned(global.id, true).unwrap();
    store.set_memory_enabled(ours.id, false).unwrap();
    store.touch_memories_last_used(&[global.id]).unwrap();
    let reloaded = store.load_memories_for_project(project.id).unwrap();
    // Pinned rows sort first.
    assert_eq!(reloaded[0].id, global.id);
    assert!(reloaded[0].pinned);
    assert!(reloaded[0].last_used_at.is_some());
    let edited = reloaded.iter().find(|memory| memory.id == ours.id).unwrap();
    assert_eq!(edited.text, "Edit tokens only.");
    assert!(!edited.enabled);

    store.delete_memory(ours.id).unwrap();
    assert_eq!(
        store.load_memories_for_project(project.id).unwrap().len(),
        1
    );
}

#[test]
fn undo_memory_deletes_the_row_and_durable_card_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let project = sample_project();
    let agent = sample_agent(&project);
    let other_agent = sample_agent(&project);
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let config = AppConfig {
        projects: vec![project.clone()],
        ..AppConfig::default()
    };
    store.save_workspace_config(&config).unwrap();
    store
        .save_agents(&[agent.clone(), other_agent.clone()])
        .unwrap();
    let memory = store
        .save_memory(
            "project",
            Some(project.id),
            "Use the shared design tokens.",
            Some(agent.id),
        )
        .unwrap();
    store
        .upsert_timeline_event(
            agent.id,
            "memorized",
            Some(format!("memorized:{}", memory.id)),
            "{}",
            memory.created_at,
        )
        .unwrap();

    assert!(store.undo_memory(other_agent.id, memory.id).is_err());
    assert_eq!(store.load_all_memories().unwrap().len(), 1);
    assert_eq!(store.load_timeline_events(agent.id).unwrap().len(), 1);

    store.undo_memory(agent.id, memory.id).unwrap();

    assert!(store.load_all_memories().unwrap().is_empty());
    assert!(store.load_timeline_events(agent.id).unwrap().is_empty());
}

#[test]
fn project_previews_are_shared_across_agents_in_the_project() {
    let dir = tempfile::tempdir().unwrap();
    let project = sample_project();
    let first = sample_agent(&project);
    let mut second = sample_agent(&project);
    second.id = Uuid::new_v4();

    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let mut config = AppConfig::default();
    config.projects = vec![project.clone()];
    store.save_workspace_config(&config).unwrap();
    store.save_agents(&[first.clone(), second.clone()]).unwrap();

    let first_open = store
        .upsert_project_preview(
            project.id,
            "http://127.0.0.1:5173",
            "Web app",
            Some(first.id),
        )
        .unwrap();
    let second_open = store
        .upsert_project_preview(project.id, "http://127.0.0.1:5173", "", Some(second.id))
        .unwrap();

    assert_eq!(first_open.id, second_open.id);
    assert_eq!(second_open.title, "Web app");
    assert_eq!(second_open.source_agent_id, Some(second.id));
    let previews = store.load_project_previews(project.id).unwrap();
    assert_eq!(previews.len(), 1);
    assert_eq!(previews[0].url, "http://127.0.0.1:5173");
}

#[test]
fn project_previews_accept_static_file_urls() {
    let dir = tempfile::tempdir().unwrap();
    let project = sample_project();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let mut config = AppConfig::default();
    config.projects = vec![project.clone()];
    store.save_workspace_config(&config).unwrap();

    let preview = store
        .upsert_project_preview(
            project.id,
            "file:///tmp/choro-static-preview/index.html",
            "Static page",
            None,
        )
        .unwrap();

    assert_eq!(preview.url, "file:///tmp/choro-static-preview/index.html");
}

#[test]
fn project_previews_are_cleared_between_desktop_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let project = sample_project();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let mut config = AppConfig::default();
    config.projects = vec![project.clone()];
    store.save_workspace_config(&config).unwrap();
    store
        .upsert_project_preview(
            project.id,
            "file:///tmp/choro-static-preview/index.html",
            "Temporary page",
            None,
        )
        .unwrap();

    store.clear_project_previews().unwrap();

    assert!(store.load_project_previews(project.id).unwrap().is_empty());
}

#[test]
fn migrates_v12_open_code_model_columns_from_v11_db() {
    let dir = tempfile::tempdir().unwrap();
    {
        let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
        store
            .rt
            .block_on(async {
                let conn = store.connect().await?;
                for column in [
                    "external_model_id",
                    "external_model_label",
                    "external_model_variants",
                ] {
                    if column_exists(&conn, "agents", column).await? {
                        conn.execute(format!("ALTER TABLE agents DROP COLUMN {column}"), ())
                            .await?;
                    }
                }
                conn.execute("DELETE FROM schema_migrations WHERE version = 12", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 13", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 14", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 15", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 16", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 17", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version >= 18", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 19", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 20", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version >= 21", ())
                    .await?;
                Ok::<_, anyhow::Error>(())
            })
            .unwrap();
    }

    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    store
        .rt
        .block_on(async {
            let conn = store.connect().await?;
            for column in [
                "external_model_id",
                "external_model_label",
                "external_model_variants",
            ] {
                assert!(column_exists(&conn, "agents", column).await?);
            }
            Ok::<_, anyhow::Error>(())
        })
        .unwrap();
}

#[test]
fn migrates_v15_solo_lane_columns_from_v14_db() {
    let dir = tempfile::tempdir().unwrap();
    {
        let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
        store
            .rt
            .block_on(async {
                let conn = store.connect().await?;
                for column in [
                    "lane_path",
                    "solo_branch",
                    "solo_base_branch",
                    "solo_rejoined_branch",
                    "lane_profile",
                ] {
                    if column_exists(&conn, "agents", column).await? {
                        conn.execute(format!("ALTER TABLE agents DROP COLUMN {column}"), ())
                            .await?;
                    }
                }
                conn.execute("DELETE FROM schema_migrations WHERE version = 15", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 16", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 17", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version >= 18", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 19", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 20", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version >= 21", ())
                    .await?;
                Ok::<_, anyhow::Error>(())
            })
            .unwrap();
    }

    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    store
        .rt
        .block_on(async {
            let conn = store.connect().await?;
            for column in [
                "lane_path",
                "solo_branch",
                "solo_base_branch",
                "solo_rejoined_branch",
                "lane_profile",
            ] {
                assert!(column_exists(&conn, "agents", column).await?);
            }
            Ok::<_, anyhow::Error>(())
        })
        .unwrap();
}

#[test]
fn migrates_v18_verification_completion_from_v17_db() {
    let dir = tempfile::tempdir().unwrap();
    {
        let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
        store
            .rt
            .block_on(async {
                let conn = store.connect().await?;
                if column_exists(&conn, "agents", "verification_completed_at").await? {
                    conn.execute(
                        "ALTER TABLE agents DROP COLUMN verification_completed_at",
                        (),
                    )
                    .await?;
                }
                conn.execute("DELETE FROM schema_migrations WHERE version >= 18", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 19", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 20", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version >= 21", ())
                    .await?;
                Ok::<_, anyhow::Error>(())
            })
            .unwrap();
    }

    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    store
        .rt
        .block_on(async {
            let conn = store.connect().await?;
            assert!(column_exists(&conn, "agents", "verification_completed_at").await?);
            Ok::<_, anyhow::Error>(())
        })
        .unwrap();
}

#[test]
fn migrates_v29_verification_closed_and_promotes_completed_agents() {
    let dir = tempfile::tempdir().unwrap();
    let project = sample_project();
    let mut agent = sample_agent(&project);
    agent.verification_completed_at = Some(42);
    agent.verification_closed = true;
    {
        let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
        let mut config = AppConfig::default();
        config.projects = vec![project];
        store.save_workspace_config(&config).unwrap();
        store.save_agents(std::slice::from_ref(&agent)).unwrap();
        store
            .rt
            .block_on(async {
                let conn = store.connect().await?;
                conn.execute("ALTER TABLE agents DROP COLUMN verification_closed", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version >= 29", ())
                    .await?;
                Ok::<_, anyhow::Error>(())
            })
            .unwrap();
    }

    let reopened = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let loaded = reopened.load_agents().unwrap();
    assert_eq!(loaded.len(), 1);
    assert!(loaded[0].verification_closed);
    assert_eq!(loaded[0].verification_completed_at, Some(42));
}

#[test]
fn migrates_v21_removes_obsolete_preview_control_table() {
    let dir = tempfile::tempdir().unwrap();
    {
        let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
        store
            .rt
            .block_on(async {
                let conn = store.connect().await?;
                conn.execute(
                    "CREATE TABLE preview_control_commands (
                        id TEXT PRIMARY KEY,
                        status TEXT NOT NULL
                    )",
                    (),
                )
                .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version >= 21", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 20", ())
                    .await?;
                conn.execute(
                    "INSERT INTO schema_migrations(version, applied_at)
                     VALUES (20, 0)",
                    (),
                )
                .await?;
                assert_eq!(schema_version(&conn).await?, 20);
                Ok::<_, anyhow::Error>(())
            })
            .unwrap();
    }

    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    store
        .rt
        .block_on(async {
            let conn = store.connect().await?;
            assert!(!table_exists(&conn, "preview_control_commands").await?);
            assert_eq!(schema_version(&conn).await?, STORE_SCHEMA_VERSION);
            Ok::<_, anyhow::Error>(())
        })
        .unwrap();
}

#[test]
fn migrates_v22_penpot_external_model_columns_from_v21_db() {
    let dir = tempfile::tempdir().unwrap();
    {
        let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
        store
            .rt
            .block_on(async {
                let conn = store.connect().await?;
                for column in [
                    "external_model_id",
                    "external_model_label",
                    "external_model_variants",
                ] {
                    if column_exists(&conn, "penpot_design_conversations", column).await? {
                        conn.execute(
                            format!("ALTER TABLE penpot_design_conversations DROP COLUMN {column}"),
                            (),
                        )
                        .await?;
                    }
                }
                conn.execute("DELETE FROM schema_migrations WHERE version >= 22", ())
                    .await?;
                assert_eq!(schema_version(&conn).await?, 21);
                Ok::<_, anyhow::Error>(())
            })
            .unwrap();
    }

    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    store
        .rt
        .block_on(async {
            let conn = store.connect().await?;
            for column in [
                "external_model_id",
                "external_model_label",
                "external_model_variants",
            ] {
                assert!(column_exists(&conn, "penpot_design_conversations", column).await?);
            }
            assert_eq!(schema_version(&conn).await?, STORE_SCHEMA_VERSION);
            Ok::<_, anyhow::Error>(())
        })
        .unwrap();
}

#[test]
fn refuses_store_from_newer_schema_version() {
    let dir = tempfile::tempdir().unwrap();
    {
        let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
        store
            .rt
            .block_on(async {
                let conn = store.connect().await?;
                record_schema_version(&conn, STORE_SCHEMA_VERSION + 1).await
            })
            .unwrap();
    }

    let error = match LocalStore::open(dir.path().to_path_buf()) {
        Ok(_) => panic!("newer schema should be rejected"),
        Err(error) => error,
    };
    let message = format!("{error:#}");
    assert!(message.contains("newer than this Choro build supports"));
    assert!(message.contains(&(STORE_SCHEMA_VERSION + 1).to_string()));
}

#[test]
fn database_provider_round_trips_and_legacy_connections_migrate_to_mongo() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = sample_project();
    project.db_connections.push(DbConnection::new_for(
        DbProvider::Supabase,
        "Supabase",
        "postgres://postgres.ref:${SUPABASE_DB_PASSWORD}@aws-0.pooler.supabase.com:5432/postgres",
    ));
    {
        let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
        let mut config = AppConfig::default();
        config.projects = vec![project.clone()];
        store.save_workspace_config(&config).unwrap();
        let loaded = store.load_workspace_config(AppConfig::default()).unwrap();
        assert_eq!(
            loaded.projects[0].db_connections[1].provider,
            DbProvider::Supabase
        );

        store
            .rt
            .block_on(async {
                let conn = store.connect().await?;
                conn.execute(
                    "DELETE FROM project_db_connections WHERE provider != 'mongodb'",
                    (),
                )
                .await?;
                conn.execute(
                    "ALTER TABLE project_db_connections DROP COLUMN provider",
                    (),
                )
                .await?;
                conn.execute(
                    "ALTER TABLE project_db_connections DROP COLUMN read_only",
                    (),
                )
                .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 11", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 12", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 13", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 14", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 15", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 16", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 17", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version >= 18", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 19", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 20", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version >= 21", ())
                    .await?;
                Ok::<_, anyhow::Error>(())
            })
            .unwrap();
    }

    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let loaded = store.load_workspace_config(AppConfig::default()).unwrap();
    assert_eq!(
        loaded.projects[0].db_connections[0].provider,
        DbProvider::MongoDb
    );
    assert!(!loaded.projects[0].db_connections[0].read_only);
}

#[test]
fn rolls_back_uncommitted_write_when_connection_closes() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();

    store
        .rt
        .block_on(async {
            let conn = store.connect().await?;
            conn.execute("BEGIN", ()).await?;
            set_meta(&conn, "interrupted-write", "must-not-survive").await?;
            drop(conn);

            let reopened = store.connect().await?;
            assert_eq!(get_meta(&reopened, "interrupted-write").await?, None);
            Ok::<_, anyhow::Error>(())
        })
        .unwrap();
}

#[test]
fn committed_write_survives_store_reopen() {
    let dir = tempfile::tempdir().unwrap();
    {
        let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
        store
            .rt
            .block_on(async {
                let conn = store.connect().await?;
                execute_transaction(&conn, |conn| {
                    Box::pin(async move { set_meta(conn, "reopen-check", "survived").await })
                })
                .await
            })
            .unwrap();
    }

    let reopened = LocalStore::open(dir.path().to_path_buf()).unwrap();
    reopened
        .rt
        .block_on(async {
            let conn = reopened.connect().await?;
            assert_eq!(
                get_meta(&conn, "reopen-check").await?.as_deref(),
                Some("survived")
            );
            Ok::<_, anyhow::Error>(())
        })
        .unwrap();
}

#[test]
fn write_transaction_cannot_be_invalidated_by_a_competing_writer() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();

    store
        .rt
        .block_on(async {
            let transaction_conn = store.connect().await?;
            let competing_store = store.clone();
            let (start_competing_write, wait_for_transaction) = tokio::sync::oneshot::channel();
            let competing_write = tokio::spawn(async move {
                wait_for_transaction.await.unwrap();
                let conn = competing_store.connect().await?;
                set_meta(&conn, "competing-write", "survived").await
            });

            execute_transaction(&transaction_conn, |conn| {
                Box::pin(async move {
                    // Establish a read snapshot before the transaction writes.
                    let _ = get_meta(conn, "transaction-write").await?;
                    start_competing_write.send(()).unwrap();
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    set_meta(conn, "transaction-write", "survived").await
                })
            })
            .await?;
            competing_write.await??;

            let verify_conn = store.connect().await?;
            assert_eq!(
                get_meta(&verify_conn, "transaction-write")
                    .await?
                    .as_deref(),
                Some("survived")
            );
            assert_eq!(
                get_meta(&verify_conn, "competing-write").await?.as_deref(),
                Some("survived")
            );
            Ok::<_, anyhow::Error>(())
        })
        .unwrap();
}

#[test]
fn abrupt_writer_process() {
    let Some(root) = std::env::var_os("CHORO_TEST_ABRUPT_WRITER_ROOT") else {
        return;
    };
    let store = LocalStore::open(PathBuf::from(root)).unwrap();
    let result: anyhow::Result<()> = store.rt.block_on(async {
        let conn = store.connect().await?;
        conn.execute("BEGIN", ()).await?;
        set_meta(&conn, "abrupt-uncommitted", "must-not-survive").await?;
        std::process::exit(91)
    });
    panic!("abrupt writer returned unexpectedly: {result:?}");
}

#[test]
fn recovers_after_process_exit_with_open_transaction() {
    let dir = tempfile::tempdir().unwrap();
    {
        let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
        store
            .rt
            .block_on(async {
                let conn = store.connect().await?;
                execute_transaction(&conn, |conn| {
                    Box::pin(async move { set_meta(conn, "abrupt-baseline", "survived").await })
                })
                .await
            })
            .unwrap();
    }

    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "local_store::tests::abrupt_writer_process",
            "--nocapture",
        ])
        .env("CHORO_TEST_ABRUPT_WRITER_ROOT", dir.path())
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(91));

    let reopened = LocalStore::open(dir.path().to_path_buf()).unwrap();
    reopened
        .rt
        .block_on(async {
            let conn = reopened.connect().await?;
            assert_eq!(
                get_meta(&conn, "abrupt-baseline").await?.as_deref(),
                Some("survived")
            );
            assert_eq!(get_meta(&conn, "abrupt-uncommitted").await?, None);
            Ok::<_, anyhow::Error>(())
        })
        .unwrap();
}

#[test]
fn migrates_legacy_product_doc_paths() {
    let dir = tempfile::tempdir().unwrap();
    let project = sample_project();
    let mut agent = sample_agent(&project);
    agent.source_doc = Some(PathBuf::from("my_ide_docs/source.md"));
    agent.linked_docs = vec![
        PathBuf::from("my_ide_docs/source.md"),
        PathBuf::from("notes/keep.md"),
    ];
    {
        let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
        let mut config = AppConfig::default();
        config.projects = vec![project];
        store.save_workspace_config(&config).unwrap();
        store.save_agents(&[agent.clone()]).unwrap();
        store
            .rt
            .block_on(async {
                let conn = store.connect().await?;
                conn.execute(
                    "UPDATE agents SET source_doc = 'my_ide_docs/source.md' WHERE id = ?1",
                    [agent.id.to_string()],
                )
                .await?;
                conn.execute(
                    "UPDATE agent_linked_docs SET path = 'my_ide_docs/source.md'
                     WHERE agent_id = ?1 AND path = 'choro_docs/source.md'",
                    [agent.id.to_string()],
                )
                .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 10", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 11", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 12", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 13", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 14", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 15", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 16", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 17", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version >= 18", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 19", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 20", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version >= 21", ())
                    .await?;
                Ok::<_, anyhow::Error>(())
            })
            .unwrap();
    }

    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let migrated = store.load_agents().unwrap().remove(0);
    assert_eq!(
        migrated.source_doc,
        Some(PathBuf::from("choro_docs/source.md"))
    );
    assert_eq!(
        migrated.linked_docs,
        vec![
            PathBuf::from("choro_docs/source.md"),
            PathBuf::from("notes/keep.md")
        ]
    );
}

#[test]
fn migrates_v5_task_tables_from_existing_db() {
    let dir = tempfile::tempdir().unwrap();
    {
        let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
        store
            .rt
            .block_on(async {
                let conn = store.connect().await?;
                conn.execute("DROP TABLE IF EXISTS project_task_tracker_connections", ())
                    .await?;
                conn.execute("DROP TABLE IF EXISTS agent_linked_tasks", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 5", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 6", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 7", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 8", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 9", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 10", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 11", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 12", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 13", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 14", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 15", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 16", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 17", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version >= 18", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 19", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 20", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version >= 21", ())
                    .await?;
                Ok::<_, anyhow::Error>(())
            })
            .unwrap();
    }

    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let mut project = sample_project();
    project
        .task_tracker_connections
        .push(sample_task_connection());
    let mut config = AppConfig::default();
    config.projects = vec![project.clone()];
    store.save_workspace_config(&config).unwrap();

    let mut agent = sample_agent(&project);
    let task = sample_task_ref();
    agent.linked_tasks = vec![task.clone()];
    agent.source_task = Some(task);
    store.save_agents(&[agent.clone()]).unwrap();

    assert_eq!(
        store
            .load_workspace_config(AppConfig::default())
            .unwrap()
            .projects,
        vec![project]
    );
    assert_eq!(store.load_agents().unwrap(), vec![agent]);
}

#[test]
fn migrates_v6_task_assignee_filter_from_v5_db() {
    let dir = tempfile::tempdir().unwrap();
    {
        let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
        store
            .rt
            .block_on(async {
                let conn = store.connect().await?;
                if column_exists(&conn, "project_task_tracker_connections", "assignee_filter")
                    .await?
                {
                    conn.execute(
                        "ALTER TABLE project_task_tracker_connections
                             DROP COLUMN assignee_filter",
                        (),
                    )
                    .await?;
                }
                conn.execute("DELETE FROM schema_migrations WHERE version = 6", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 7", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 8", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 9", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 10", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 11", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 12", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 13", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 14", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 15", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 16", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 17", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version >= 18", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 19", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 20", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version >= 21", ())
                    .await?;
                Ok::<_, anyhow::Error>(())
            })
            .unwrap();
    }

    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    store
        .rt
        .block_on(async {
            let conn = store.connect().await?;
            assert!(
                column_exists(&conn, "project_task_tracker_connections", "assignee_filter").await?
            );
            Ok::<_, anyhow::Error>(())
        })
        .unwrap();

    let mut project = sample_project();
    let mut connection = sample_task_connection();
    connection.assignee_filter = Some("currentUser()".to_string());
    connection.assignee_account_id = None;
    connection.assignee_display_name = None;
    project.task_tracker_connections.push(connection);
    let mut config = AppConfig::default();
    config.projects = vec![project.clone()];
    store.save_workspace_config(&config).unwrap();

    let loaded = store.load_workspace_config(AppConfig::default()).unwrap();
    assert_eq!(
        loaded.projects[0].task_tracker_connections[0]
            .assignee_filter
            .as_deref(),
        Some("currentUser()")
    );
}

#[test]
fn migrates_v7_task_assignee_user_columns_from_v6_db() {
    let dir = tempfile::tempdir().unwrap();
    {
        let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
        store
            .rt
            .block_on(async {
                let conn = store.connect().await?;
                if column_exists(
                    &conn,
                    "project_task_tracker_connections",
                    "assignee_account_id",
                )
                .await?
                {
                    conn.execute(
                        "ALTER TABLE project_task_tracker_connections
                             DROP COLUMN assignee_account_id",
                        (),
                    )
                    .await?;
                }
                if column_exists(
                    &conn,
                    "project_task_tracker_connections",
                    "assignee_display_name",
                )
                .await?
                {
                    conn.execute(
                        "ALTER TABLE project_task_tracker_connections
                             DROP COLUMN assignee_display_name",
                        (),
                    )
                    .await?;
                }
                conn.execute("DELETE FROM schema_migrations WHERE version = 7", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 8", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 9", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 10", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 11", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 12", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 13", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 14", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 15", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 16", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 17", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version >= 18", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 19", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 20", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version >= 21", ())
                    .await?;
                Ok::<_, anyhow::Error>(())
            })
            .unwrap();
    }

    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    store
        .rt
        .block_on(async {
            let conn = store.connect().await?;
            assert!(
                column_exists(
                    &conn,
                    "project_task_tracker_connections",
                    "assignee_account_id"
                )
                .await?
            );
            assert!(
                column_exists(
                    &conn,
                    "project_task_tracker_connections",
                    "assignee_display_name"
                )
                .await?
            );
            Ok::<_, anyhow::Error>(())
        })
        .unwrap();

    let mut project = sample_project();
    project
        .task_tracker_connections
        .push(sample_task_connection());
    let mut config = AppConfig::default();
    config.projects = vec![project.clone()];
    store.save_workspace_config(&config).unwrap();

    let loaded = store.load_workspace_config(AppConfig::default()).unwrap();
    let connection = &loaded.projects[0].task_tracker_connections[0];
    assert_eq!(
        connection.assignee_account_id.as_deref(),
        Some("712020:abc-123")
    );
    assert_eq!(
        connection.assignee_display_name.as_deref(),
        Some("Ada Lovelace")
    );
}

#[test]
fn migrates_v8_provider_sources_and_personal_tasks() {
    let dir = tempfile::tempdir().unwrap();
    {
        let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
        store
            .rt
            .block_on(async {
                let conn = store.connect().await?;
                for column in [
                    "source_id",
                    "source_name",
                    "source_kind",
                    "provider_config_json",
                    "filters_json",
                ] {
                    if column_exists(&conn, "project_task_tracker_connections", column).await? {
                        conn.execute(
                            format!(
                                "ALTER TABLE project_task_tracker_connections DROP COLUMN {column}"
                            ),
                            (),
                        )
                        .await?;
                    }
                }
                conn.execute("DROP TABLE IF EXISTS personal_tasks", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 8", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 9", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 10", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 11", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 12", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 13", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 14", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 15", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 16", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 17", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version >= 18", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 19", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version = 20", ())
                    .await?;
                conn.execute("DELETE FROM schema_migrations WHERE version >= 21", ())
                    .await?;
                Ok::<_, anyhow::Error>(())
            })
            .unwrap();
    }

    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    store
        .rt
        .block_on(async {
            let conn = store.connect().await?;
            assert!(column_exists(&conn, "project_task_tracker_connections", "source_id").await?);
            assert!(table_count(&conn, "personal_tasks").await?.eq(&0));
            Ok::<_, anyhow::Error>(())
        })
        .unwrap();

    let project = sample_project();
    let mut config = AppConfig::default();
    config.projects = vec![project.clone()];
    store.save_workspace_config(&config).unwrap();
    let task = store
        .create_personal_task(project.id, "Write spec", "- [ ] Build it")
        .unwrap();
    assert_eq!(task.key_number, 1);
    assert_eq!(task.issue_key(), "TASK-1");
    let loaded = store.load_personal_tasks(project.id).unwrap();
    assert_eq!(loaded, vec![task]);
}

#[test]
fn personal_task_comments_and_status_updates() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let project = sample_project();
    let mut config = AppConfig::default();
    config.projects = vec![project.clone()];
    store.save_workspace_config(&config).unwrap();

    let task = store
        .create_personal_task(project.id, "Ship it", "body")
        .unwrap();
    assert_eq!(task.status, PersonalTaskStatus::Todo);
    assert!(store
        .load_personal_task_comments(task.id)
        .unwrap()
        .is_empty());

    let comment = store
        .add_personal_task_comment(task.id, "Agent", "Shipped a PR: https://example/pr/1")
        .unwrap();
    let comments = store.load_personal_task_comments(task.id).unwrap();
    assert_eq!(comments, vec![comment]);
    assert_eq!(comments[0].author, "Agent");
    assert!(comments[0].body.contains("https://example/pr/1"));

    store
        .set_personal_task_status(task.id, PersonalTaskStatus::Done)
        .unwrap();
    let reloaded = store.load_personal_tasks(project.id).unwrap();
    assert_eq!(reloaded[0].status, PersonalTaskStatus::Done);
}

#[test]
fn saves_and_loads_workspace_and_agents() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let project = sample_project();
    let section = ProjectSection::new("Work");
    let mut config = AppConfig::default();
    config.project_sections = vec![section];
    config.projects = vec![project.clone()];
    config.active_project = Some(project.id);
    store.save_workspace_config(&config).unwrap();
    let loaded = store.load_workspace_config(AppConfig::default()).unwrap();
    assert_eq!(loaded.projects, config.projects);

    let agent = sample_agent(&project);
    store.save_agents(&[agent.clone()]).unwrap();
    let loaded_agents = store.load_agents().unwrap();
    assert_eq!(loaded_agents, vec![agent]);
}

#[test]
fn saves_and_loads_open_code_model_identity() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let project = sample_project();
    let mut config = AppConfig::default();
    config.projects = vec![project.clone()];
    store.save_workspace_config(&config).unwrap();

    let mut agent = sample_agent(&project);
    agent.set_external_model(
        "opencode/big-pickle",
        "Big Pickle",
        vec!["low".into(), "high".into()],
    );
    store.save_agents(&[agent.clone()]).unwrap();
    assert_eq!(store.load_agents().unwrap(), vec![agent]);
}

#[test]
fn saves_and_loads_task_tracker_connections_and_agent_task_links() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let mut project = sample_project();
    project
        .task_tracker_connections
        .push(sample_task_connection());
    let mut config = AppConfig::default();
    config.projects = vec![project.clone()];
    store.save_workspace_config(&config).unwrap();

    let mut agent = sample_agent(&project);
    let task = sample_task_ref();
    agent.linked_tasks = vec![task.clone()];
    agent.source_task = Some(task.clone());
    store.save_agents(&[agent.clone()]).unwrap();

    let loaded_config = store.load_workspace_config(AppConfig::default()).unwrap();
    assert_eq!(
        loaded_config.projects[0].task_tracker_connections,
        project.task_tracker_connections
    );
    let loaded_agent = store.load_agents().unwrap().pop().unwrap();
    assert_eq!(loaded_agent.linked_tasks, vec![task.clone()]);
    assert_eq!(loaded_agent.source_task, Some(task));
}

#[test]
fn materializes_agent_attachment() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let agent_id = Uuid::new_v4();
    let attachment = store
        .materialize_attachment_bytes(
            agent_id,
            "hello.txt",
            Some("text/plain".into()),
            "txt",
            b"hello",
        )
        .unwrap();
    assert!(dir.path().join(&attachment.relative_path).is_file());
    assert_eq!(store.load_attachments(agent_id).unwrap(), vec![attachment]);
}

#[test]
fn creates_loads_and_deletes_project_reference() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let project = sample_project();
    let mut config = AppConfig::default();
    config.projects = vec![project.clone()];
    store.save_workspace_config(&config).unwrap();
    let source = dir.path().join("mockup.png");
    write_test_png(&source);

    let reference = store
        .create_project_reference(
            project.id,
            ProjectReferenceKind::Image,
            "Settings mockup",
            source.to_string_lossy().to_string(),
            "Use layout only",
            Some(&source),
        )
        .unwrap();
    assert_eq!(reference.kind, ProjectReferenceKind::Image);
    assert!(PathBuf::from(&reference.source).starts_with("data/projects"));
    let preview = reference.preview_relative_path.as_ref().unwrap();
    assert!(dir.path().join(preview).is_file());
    assert!(dir.path().join(&reference.source).is_file());

    let loaded = store.load_project_references(project.id).unwrap();
    assert_eq!(loaded, vec![reference.clone()]);

    store.delete_project_reference(&reference).unwrap();
    assert!(store
        .load_project_references(project.id)
        .unwrap()
        .is_empty());
    assert!(!store
        .project_reference_dir(project.id, reference.id)
        .exists());
}

#[test]
fn workspace_save_preserves_project_references() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let mut project = sample_project();
    let mut config = AppConfig::default();
    config.projects = vec![project.clone()];
    store.save_workspace_config(&config).unwrap();

    let source = dir.path().join("mockup.png");
    write_test_png(&source);
    let reference = store
        .create_project_reference(
            project.id,
            ProjectReferenceKind::Image,
            "Settings mockup",
            source.to_string_lossy().to_string(),
            "",
            Some(&source),
        )
        .unwrap();

    project.name = "Renamed app".to_string();
    config.projects = vec![project.clone()];
    store.save_workspace_config(&config).unwrap();

    let reopened = LocalStore::open(dir.path().to_path_buf()).unwrap();
    assert_eq!(
        reopened.load_project_references(project.id).unwrap(),
        vec![reference]
    );
}

#[test]
fn project_reference_preview_rejects_invalid_image() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let project = sample_project();
    let invalid = dir.path().join("not-an-image.txt");
    fs::write(&invalid, "nope").unwrap();

    let error = store
        .create_project_reference(
            project.id,
            ProjectReferenceKind::Url,
            "Broken preview",
            "https://example.com",
            "",
            Some(&invalid),
        )
        .unwrap_err();
    assert!(format!("{error:#}").contains("reference preview"));
}

#[test]
fn saves_and_loads_agent_diff_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let project = sample_project();
    let agent = sample_agent(&project);
    let diff = sample_diff("src/lib.rs");

    let snapshot = store
        .create_agent_diff_snapshot(
            agent.id,
            project.id,
            project.path.clone(),
            "unit_test",
            Some("base".to_string()),
            None,
            None,
            vec![diff.clone()],
        )
        .unwrap();

    let loaded = store
        .load_agent_diff_snapshot(snapshot.id)
        .unwrap()
        .expect("snapshot should exist");
    assert_eq!(loaded.source, "unit_test");
    assert_eq!(loaded.files.len(), 1);
    assert_eq!(loaded.files[0].additions, 2);
    assert_eq!(loaded.files[0].deletions, 1);
    assert_eq!(loaded.files[0].diff, diff);
}

#[test]
fn export_import_round_trip() {
    let source = tempfile::tempdir().unwrap();
    let source_store = LocalStore::open(source.path().to_path_buf()).unwrap();
    let mut project = sample_project();
    project
        .task_tracker_connections
        .push(sample_task_connection());
    let mut config = AppConfig::default();
    config.projects = vec![project.clone()];
    config.active_project = Some(project.id);
    source_store.save_workspace_config(&config).unwrap();
    let mut agent = sample_agent(&project);
    let task = sample_task_ref();
    agent.linked_tasks = vec![task.clone()];
    agent.source_task = Some(task);
    source_store.save_agents(&[agent.clone()]).unwrap();
    let memory = source_store
        .save_memory(
            "project",
            Some(project.id),
            "Keep export-compatible memory.",
            Some(agent.id),
        )
        .unwrap();
    source_store
        .append_chat_message(agent.id, "user", "hello", 1, None)
        .unwrap();
    source_store
        .upsert_timeline_event(
            agent.id,
            "message",
            Some("message:user:hello".into()),
            r#"{"type":"message","role":"user","text":"hello","created_at":1}"#,
            1,
        )
        .unwrap();
    let file_ledger = vec![StoredChatFileLedgerEntry {
            segments_json: "[]".into(),
        counts_unavailable: false,
        agent_id: agent.id,
        path: PathBuf::from("src/lib.rs"),
        observed: false,
        additions: 1,
        deletions: 1,
        baseline_hash: Some("before".into()),
        result_hash: Some("after".into()),
        baseline_content: Some("before\n".into()),
        result_content: Some("after\n".into()),
        updated_at: 2,
    }];
    source_store
        .replace_chat_file_ledger(agent.id, 1, &file_ledger)
        .unwrap();
    source_store
        .materialize_attachment_bytes(agent.id, "hello.txt", None, "txt", b"hello")
        .unwrap();
    let reference_source = source.path().join("reference.png");
    write_test_png(&reference_source);
    let reference = source_store
        .create_project_reference(
            project.id,
            ProjectReferenceKind::Image,
            "Reference",
            reference_source.to_string_lossy().to_string(),
            "Use this",
            Some(&reference_source),
        )
        .unwrap();
    let diff_snapshot = source_store
        .create_agent_diff_snapshot(
            agent.id,
            project.id,
            project.path.clone(),
            "unit_test",
            None,
            None,
            Some("abc123".to_string()),
            vec![sample_diff("src/lib.rs")],
        )
        .unwrap();
    let orbit_module = source_store
        .save_orbit_module(&analytics_orbit_template())
        .unwrap();
    source_store
        .set_project_orbit_module_enabled(project.id, OrbitModuleId::Custom(orbit_module.id), true)
        .unwrap();
    let orbit_invocation = source_store
        .create_orbit_invocation(agent.id, project.id, orbit_module.id)
        .unwrap();
    source_store
        .apply_orbit_invocation_changes(
            orbit_invocation.id,
            agent.id,
            project.id,
            0,
            vec![OrbitRecordInput {
                id: None,
                section: Some("Onboarding".into()),
                values: BTreeMap::from([
                    ("name".into(), serde_json::json!("signup_started")),
                    (
                        "what_it_does".into(),
                        serde_json::json!("Fires when signup starts"),
                    ),
                    ("properties".into(), serde_json::json!(["source"])),
                    ("notes".into(), serde_json::json!("")),
                ]),
            }],
            Vec::new(),
        )
        .unwrap();
    source_store
        .complete_orbit_invocation(orbit_invocation.id, agent.id)
        .unwrap();
    let archive = source.path().join("export.zip");
    source_store.export_workspace(&archive).unwrap();

    let target = tempfile::tempdir().unwrap();
    let target_store = LocalStore::open(target.path().to_path_buf()).unwrap();
    target_store
        .save_memory("global", None, "Stale destination memory.", None)
        .unwrap();
    target_store.import_workspace_replace(&archive).unwrap();
    assert_eq!(
        target_store
            .load_workspace_config(AppConfig::default())
            .unwrap()
            .projects,
        vec![project.clone()]
    );
    assert_eq!(target_store.load_agents().unwrap(), vec![agent.clone()]);
    assert_eq!(target_store.load_chat_messages(agent.id).unwrap().len(), 1);
    assert_eq!(
        target_store.load_timeline_events(agent.id).unwrap().len(),
        1
    );
    assert_eq!(
        target_store
            .load_chat_file_ledger(agent.id)
            .unwrap()
            .unwrap()
            .entries,
        file_ledger
    );
    assert_eq!(target_store.load_all_memories().unwrap(), vec![memory]);
    assert_eq!(target_store.load_attachments(agent.id).unwrap().len(), 1);
    let imported_refs = target_store.load_project_references(project.id).unwrap();
    assert_eq!(imported_refs.len(), 1);
    assert_eq!(imported_refs[0].title, reference.title);
    assert!(target
        .path()
        .join(imported_refs[0].preview_relative_path.as_ref().unwrap())
        .is_file());
    assert!(target.path().join(&imported_refs[0].source).is_file());
    assert_eq!(
        target_store
            .load_agent_diff_snapshot(diff_snapshot.id)
            .unwrap()
            .expect("diff snapshot should import")
            .commit_sha
            .as_deref(),
        Some("abc123")
    );
    assert_eq!(
        target_store
            .load_orbit_module(orbit_module.id)
            .unwrap()
            .unwrap()
            .name,
        "Analytics"
    );
    assert!(target_store
        .load_project_orbit_bindings(project.id)
        .unwrap()
        .iter()
        .any(|binding| {
            binding.module == OrbitModuleId::Custom(orbit_module.id) && binding.enabled
        }));
    assert_eq!(
        target_store
            .load_orbit_records(project.id, orbit_module.id)
            .unwrap()
            .len(),
        1
    );
    let imported_update = target_store
        .load_orbit_invocation_update(orbit_invocation.id, agent.id)
        .unwrap()
        .expect("Orbit update should remain actionable after import");
    assert_eq!(imported_update.inserted, 1);
    target_store
        .undo_orbit_invocation(orbit_invocation.id, agent.id)
        .unwrap();
    assert!(target_store
        .load_orbit_records(project.id, orbit_module.id)
        .unwrap()
        .is_empty());
}

#[test]
fn backfills_changed_files_from_unique_commit() {
    let repo_dir = tempfile::tempdir().unwrap();
    let repo = repo_with_commit(repo_dir.path());
    fs::write(workdir(&repo).join("README.md"), "one\ntwo\nthree\n").unwrap();
    let commit = commit_all(&repo, "rewrite readme").to_string();

    let store_dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(store_dir.path().to_path_buf()).unwrap();
    let project = Project::from_path(repo_dir.path().to_path_buf());
    let mut config = AppConfig::default();
    config.projects = vec![project.clone()];
    store.save_workspace_config(&config).unwrap();
    let agent = sample_agent(&project);
    store.save_agents(&[agent.clone()]).unwrap();
    let payload = serde_json::json!({
        "type": "changed_files",
        "files": [{
            "path": "README.md",
            "additions": 3,
            "deletions": 1
        }]
    });
    store
        .upsert_timeline_event(
            agent.id,
            "changed_files",
            Some("changed_files:readme".to_string()),
            payload.to_string(),
            7,
        )
        .unwrap();

    let counts = store.backfill_agent_diff_snapshots().unwrap();
    assert_eq!(counts.backfilled_events, 1);
    assert_eq!(counts.commit_matches, 1);
    let event = store.load_timeline_events(agent.id).unwrap().pop().unwrap();
    let updated: serde_json::Value = serde_json::from_str(&event.payload_json).unwrap();
    let snapshot_id = updated
        .get("snapshot_id")
        .and_then(serde_json::Value::as_str)
        .and_then(|id| Uuid::parse_str(id).ok())
        .expect("snapshot id should be attached");
    assert_eq!(
        updated
            .get("commit_sha")
            .and_then(serde_json::Value::as_str),
        Some(commit.as_str())
    );
    let snapshot = store
        .load_agent_diff_snapshot(snapshot_id)
        .unwrap()
        .expect("attached snapshot should load");
    assert_eq!(snapshot.commit_sha.as_deref(), Some(commit.as_str()));
    assert_eq!(snapshot.files[0].path, PathBuf::from("README.md"));
}

#[test]
fn backend_message_upsert_keeps_fuller_stream_text() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let agent_id = Uuid::new_v4();
    let backend_id = Some("msg_123".to_string());

    store
        .upsert_chat_message(
            agent_id,
            "assistant",
            "Full answer with matched rows.",
            1,
            backend_id.clone(),
        )
        .unwrap();
    store
        .upsert_chat_message(agent_id, "assistant", " rows.", 2, backend_id.clone())
        .unwrap();
    let messages = store.load_chat_messages(agent_id).unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].text, "Full answer with matched rows.");

    let full_payload = serde_json::json!({
        "type": "message",
        "role": "assistant",
        "text": "Full answer with matched rows.",
        "created_at": 1,
        "backend_message_id": "msg_123"
    });
    let suffix_payload = serde_json::json!({
        "type": "message",
        "role": "assistant",
        "text": " rows.",
        "created_at": 2,
        "backend_message_id": "msg_123"
    });
    store
        .upsert_timeline_event(
            agent_id,
            "message",
            Some("message:assistant:backend:msg_123".to_string()),
            full_payload.to_string(),
            1,
        )
        .unwrap();
    store
        .upsert_timeline_event(
            agent_id,
            "message",
            Some("message:assistant:backend:msg_123".to_string()),
            suffix_payload.to_string(),
            2,
        )
        .unwrap();
    let events = store.load_timeline_events(agent_id).unwrap();
    assert_eq!(events.len(), 1);
    let payload: serde_json::Value = serde_json::from_str(&events[0].payload_json).unwrap();
    assert_eq!(
        payload.get("text").and_then(serde_json::Value::as_str),
        Some("Full answer with matched rows.")
    );
}

#[test]
fn timeline_pages_walk_backwards_without_overlap() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let agent_id = Uuid::new_v4();
    let other_agent_id = Uuid::new_v4();

    for index in 0..7 {
        store
            .upsert_timeline_event(
                agent_id,
                "work_log",
                Some(format!("event-{index}")),
                format!(r#"{{"index":{index}}}"#),
                index,
            )
            .unwrap();
    }
    store
        .upsert_timeline_event(
            other_agent_id,
            "work_log",
            Some("other-event".to_string()),
            "{}".to_string(),
            99,
        )
        .unwrap();

    let latest = store.load_timeline_events_page(agent_id, None, 3).unwrap();
    assert_eq!(
        latest
            .events
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        vec![4, 5, 6]
    );
    assert_eq!(latest.oldest_sequence, Some(4));
    assert!(latest.has_more);

    let middle = store
        .load_timeline_events_page(agent_id, latest.oldest_sequence, 3)
        .unwrap();
    assert_eq!(
        middle
            .events
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert_eq!(middle.oldest_sequence, Some(1));
    assert!(middle.has_more);

    let oldest = store
        .load_timeline_events_page(agent_id, middle.oldest_sequence, 3)
        .unwrap();
    assert_eq!(
        oldest
            .events
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        vec![0]
    );
    assert_eq!(oldest.oldest_sequence, Some(0));
    assert!(!oldest.has_more);
}

#[test]
fn timeline_message_search_candidates_use_folded_text_and_stay_agent_scoped() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let agent_id = Uuid::new_v4();
    let other_agent_id = Uuid::new_v4();

    let events = [
        (
            agent_id,
            "first",
            serde_json::json!({
                "type": "message",
                "role": "assistant",
                "text": "Die Maße sind korrekt.",
                "search_text_version": 1,
                "search_text": "die masse sind korrekt.",
                "created_at": 1,
                "backend_message_id": "a-1"
            }),
        ),
        (
            agent_id,
            "second",
            serde_json::json!({
                "type": "message",
                "role": "user",
                "text": "hidden connected context performance token",
                "display_text": "Please check PERFORMANCE before shipping.",
                "search_text_version": 1,
                "search_text": "please check performance before shipping.",
                "created_at": 2,
                "backend_message_id": null
            }),
        ),
        (
            other_agent_id,
            "other",
            serde_json::json!({
                "type": "message",
                "role": "assistant",
                "text": "performance in another conversation",
                "search_text_version": 1,
                "search_text": "performance in another conversation",
                "created_at": 3,
                "backend_message_id": "other-1"
            }),
        ),
    ];
    for (owner, key, payload) in events {
        store
            .upsert_timeline_event(
                owner,
                "message",
                Some(key.to_string()),
                payload.to_string(),
                1,
            )
            .unwrap();
    }

    let performance = store
        .search_timeline_message_candidates_page(agent_id, "performance", 1, None, 20)
        .unwrap();
    // The user row is present both as a real match and as the delimiter the
    // caller uses to decide whether following assistant rows are visible.
    assert_eq!(performance.events.len(), 1);
    assert!(!performance.has_more);

    // Unicode folding happens before SQL sees either side, so SQLite's
    // ASCII-only lower() behavior is not part of matching.
    let unicode = store
        .search_timeline_message_candidates_page(agent_id, "masse", 1, None, 20)
        .unwrap();
    assert_eq!(unicode.events.len(), 2);
    assert!(unicode.events[0].sequence < unicode.events[1].sequence);

    let other = store
        .search_timeline_message_candidates_page(agent_id, "another conversation", 1, None, 20)
        .unwrap();
    assert_eq!(
        other.events.len(),
        1,
        "only the local user delimiter remains"
    );
}

#[test]
fn timeline_message_search_keeps_legacy_payloads_as_filter_candidates() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let agent_id = Uuid::new_v4();

    store
        .upsert_timeline_event(
            agent_id,
            "message",
            Some("legacy".to_string()),
            serde_json::json!({
                "type": "message",
                "role": "assistant",
                "text": "An older payload without a search index.",
                "created_at": 1,
                "backend_message_id": "legacy-1"
            })
            .to_string(),
            1,
        )
        .unwrap();

    let page = store
        .search_timeline_message_candidates_page(agent_id, "not present", 1, None, 20)
        .unwrap();
    assert_eq!(page.events.len(), 1);
}

#[test]
fn scale_query_shape_handles_large_sets() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let project = sample_project();
    let mut config = AppConfig::default();
    config.projects = vec![project.clone()];
    store.save_workspace_config(&config).unwrap();
    let mut agents = Vec::new();
    for ix in 0..5_000 {
        let mut agent = sample_agent(&project);
        agent.id = Uuid::new_v4();
        agent.title = format!("Agent {ix}");
        agent.updated_at = ix;
        agents.push(agent);
    }
    store.save_agents(&agents).unwrap();
    let loaded = store.load_agents().unwrap();
    assert_eq!(loaded.len(), 5_000);
    assert!(loaded
        .windows(2)
        .all(|pair| pair[0].updated_at >= pair[1].updated_at));
}

#[test]
fn penpot_conversations_are_scoped_to_one_design_and_keep_history() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let project = sample_project();
    let mut config = AppConfig::default();
    config.projects = vec![project.clone()];
    store.save_workspace_config(&config).unwrap();

    let now = crate::agents::unix_now();
    let connection = StoredPenpotConnection {
        id: Uuid::new_v4(),
        instance_url: "https://design.penpot.app".to_string(),
        mcp_url: "https://design.penpot.app/mcp/stream".to_string(),
        profile_id: Some(Uuid::new_v4()),
        profile_email: Some("designer@example.com".to_string()),
        default_team_id: Some(Uuid::new_v4()),
        default_project_id: Some(Uuid::new_v4()),
        is_active: true,
        verified_at: Some(now),
        created_at: now,
        updated_at: now,
    };
    store.save_active_penpot_connection(&connection).unwrap();
    let design = StoredPenpotDesign {
        id: Uuid::new_v4(),
        project_id: project.id,
        connection_id: connection.id,
        penpot_file_id: Uuid::new_v4(),
        penpot_project_id: connection.default_project_id.unwrap(),
        penpot_team_id: connection.default_team_id.unwrap(),
        name: "Checkout".to_string(),
        page_id: Some(Uuid::new_v4()),
        source_doc: Some(PathBuf::from("choro_docs/checkout.md")),
        source_task: None,
        last_synced_at: Some(now),
        archived_at: None,
        created_at: now,
        updated_at: now,
    };
    store.upsert_penpot_design(&design).unwrap();
    store
        .upsert_project_penpot_binding(&StoredProjectPenpotBinding {
            project_id: project.id,
            connection_id: connection.id,
            penpot_team_id: design.penpot_team_id,
            penpot_project_id: design.penpot_project_id,
            selected_design_id: Some(design.id),
            created_at: now,
            updated_at: now,
        })
        .unwrap();
    assert_eq!(
        store.load_penpot_designs(project.id).unwrap(),
        vec![design.clone()]
    );

    let first = store.ensure_current_penpot_conversation(design.id).unwrap();
    let second = store.create_penpot_conversation(design.id).unwrap();
    assert_ne!(first.agent_id, second.agent_id);
    assert_eq!(second.ordinal, first.ordinal + 1);

    let conversations = store.load_penpot_conversations(design.id).unwrap();
    assert_eq!(conversations.len(), 2);
    assert_eq!(
        conversations
            .iter()
            .filter(|conversation| conversation.is_current)
            .count(),
        1
    );
    assert_eq!(
        conversations
            .iter()
            .find(|conversation| conversation.is_current)
            .map(|conversation| conversation.id),
        Some(second.id)
    );

    let external_variants = vec!["low".to_string(), "high".to_string()];
    store
        .update_penpot_conversation_runtime(
            second.agent_id,
            AgentKind::OpenCode,
            AgentModel::OpenCode,
            Some("openai/gpt-5.4"),
            Some("GPT-5.4"),
            &external_variants,
            AgentEffort::High,
            AgentAccessMode::FullAccess,
            None,
            None,
            None,
        )
        .unwrap();
    let updated = store
        .load_penpot_conversations(design.id)
        .unwrap()
        .into_iter()
        .find(|conversation| conversation.id == second.id)
        .unwrap();
    assert_eq!(updated.provider, AgentKind::OpenCode);
    assert_eq!(updated.external_model_id.as_deref(), Some("openai/gpt-5.4"));
    assert_eq!(updated.external_model_label.as_deref(), Some("GPT-5.4"));
    assert_eq!(updated.external_model_variants, external_variants);
    assert_eq!(updated.effort, AgentEffort::High);

    store
        .select_penpot_conversation(design.id, first.id)
        .unwrap();
    assert_eq!(
        store
            .ensure_current_penpot_conversation(design.id)
            .unwrap()
            .id,
        first.id
    );

    store.delete_penpot_design(project.id, design.id).unwrap();
    assert!(store.load_penpot_designs(project.id).unwrap().is_empty());
    assert!(store
        .load_penpot_conversations(design.id)
        .unwrap()
        .is_empty());
    assert_eq!(
        store
            .project_penpot_binding(project.id, connection.id)
            .unwrap()
            .unwrap()
            .selected_design_id,
        None
    );
}

#[test]
fn changing_active_penpot_account_does_not_reassign_existing_designs() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let project = sample_project();
    let mut config = AppConfig::default();
    config.projects = vec![project.clone()];
    store.save_workspace_config(&config).unwrap();
    let now = crate::agents::unix_now();

    let connection_a = StoredPenpotConnection {
        id: Uuid::new_v4(),
        instance_url: "https://design.penpot.app".to_string(),
        mcp_url: "https://design.penpot.app/mcp/stream".to_string(),
        profile_id: Some(Uuid::new_v4()),
        profile_email: Some("a@example.com".to_string()),
        default_team_id: Some(Uuid::new_v4()),
        default_project_id: Some(Uuid::new_v4()),
        is_active: true,
        verified_at: Some(now),
        created_at: now,
        updated_at: now,
    };
    store.save_active_penpot_connection(&connection_a).unwrap();
    let design = StoredPenpotDesign {
        id: Uuid::new_v4(),
        project_id: project.id,
        connection_id: connection_a.id,
        penpot_file_id: Uuid::new_v4(),
        penpot_project_id: connection_a.default_project_id.unwrap(),
        penpot_team_id: connection_a.default_team_id.unwrap(),
        name: "Account A file".to_string(),
        page_id: None,
        source_doc: None,
        source_task: Some(sample_task_ref()),
        last_synced_at: None,
        archived_at: None,
        created_at: now,
        updated_at: now,
    };
    store.upsert_penpot_design(&design).unwrap();

    let mut connection_b = connection_a.clone();
    connection_b.id = Uuid::new_v4();
    connection_b.profile_id = Some(Uuid::new_v4());
    connection_b.profile_email = Some("b@example.com".to_string());
    store.save_active_penpot_connection(&connection_b).unwrap();

    assert_eq!(
        store.active_penpot_connection().unwrap().unwrap().id,
        connection_b.id
    );
    assert_eq!(
        store.load_penpot_designs(project.id).unwrap()[0].connection_id,
        connection_a.id
    );
    assert!(store.load_penpot_designs(project.id).unwrap()[0]
        .source_task
        .as_ref()
        .is_some_and(|task| task.same_issue(&sample_task_ref())));
    let reconnected = store
        .penpot_connection_for_profile(&connection_a.instance_url, connection_a.profile_id.unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(reconnected.id, connection_a.id);
}

#[test]
fn current_schema_repairs_missing_penpot_tables() {
    let dir = tempfile::tempdir().unwrap();
    {
        let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
        store
            .rt
            .block_on(async {
                let conn = store.connect().await?;
                conn.execute("DROP TABLE penpot_design_conversations", ())
                    .await?;
                conn.execute("DROP TABLE penpot_designs", ()).await?;
                conn.execute("DROP TABLE project_penpot_bindings", ())
                    .await?;
                conn.execute("DROP TABLE penpot_connections", ()).await?;
                assert_eq!(schema_version(&conn).await?, STORE_SCHEMA_VERSION);
                Ok::<_, anyhow::Error>(())
            })
            .unwrap();
    }

    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let now = crate::agents::unix_now();
    let connection = StoredPenpotConnection {
        id: Uuid::new_v4(),
        instance_url: "https://design.penpot.app".to_string(),
        mcp_url: "https://design.penpot.app/mcp/stream".to_string(),
        profile_id: Some(Uuid::new_v4()),
        profile_email: Some("repaired@example.com".to_string()),
        default_team_id: Some(Uuid::new_v4()),
        default_project_id: Some(Uuid::new_v4()),
        is_active: true,
        verified_at: Some(now),
        created_at: now,
        updated_at: now,
    };
    store.save_active_penpot_connection(&connection).unwrap();
    assert_eq!(
        store.active_penpot_connection().unwrap().unwrap().id,
        connection.id
    );
}

#[test]
fn orbit_modules_are_global_but_records_are_project_scoped() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let project_a = Project::from_path(PathBuf::from("/tmp/choro-orbit-a"));
    let project_b = Project::from_path(PathBuf::from("/tmp/choro-orbit-b"));
    let mut config = AppConfig::default();
    config.projects = vec![project_a.clone(), project_b.clone()];
    store.save_workspace_config(&config).unwrap();

    for project in [&project_a, &project_b] {
        let bindings = store.load_project_orbit_bindings(project.id).unwrap();
        assert_eq!(bindings.len(), 2);
        assert!(bindings.iter().all(|binding| binding.enabled));
    }

    let module = store
        .save_orbit_module(&analytics_orbit_template())
        .unwrap();
    store
        .set_project_orbit_module_enabled(project_a.id, OrbitModuleId::Custom(module.id), true)
        .unwrap();
    let values = BTreeMap::from([
        ("name".into(), serde_json::json!("onboarding_started")),
        (
            "what_it_does".into(),
            serde_json::json!("Fires when onboarding begins."),
        ),
        (
            "properties".into(),
            serde_json::json!(["source — entry point"]),
        ),
        ("notes".into(), serde_json::json!("")),
    ]);
    store
        .save_orbit_record(
            project_a.id,
            module.id,
            OrbitRecordInput {
                id: None,
                section: Some("Onboarding".into()),
                values,
            },
        )
        .unwrap();

    assert_eq!(
        store
            .load_orbit_records(project_a.id, module.id)
            .unwrap()
            .len(),
        1
    );
    assert!(store
        .load_orbit_records(project_b.id, module.id)
        .unwrap()
        .is_empty());
    let snapshot = store
        .load_orbit_snapshot(&[project_a.id, project_b.id])
        .unwrap();
    assert!(snapshot.modules.iter().any(|loaded| loaded.id == module.id));
    assert_eq!(snapshot.bindings.len(), 2);
    assert_eq!(
        snapshot
            .records
            .get(&(project_a.id, module.id))
            .map(Vec::len),
        Some(1),
    );
    assert!(!snapshot.records.contains_key(&(project_b.id, module.id)));
    store
        .set_project_orbit_module_enabled(project_a.id, OrbitModuleId::Custom(module.id), false)
        .unwrap();
    assert_eq!(
        store
            .load_orbit_records(project_a.id, module.id)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn orbit_agent_updates_are_scoped_revisioned_and_undoable() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let project = Project::from_path(PathBuf::from("/tmp/choro-orbit-agent"));
    let agent = sample_agent(&project);
    let mut config = AppConfig::default();
    config.projects = vec![project.clone()];
    store.save_workspace_config(&config).unwrap();
    store.save_agents(std::slice::from_ref(&agent)).unwrap();
    let module = store
        .save_orbit_module(&analytics_orbit_template())
        .unwrap();
    store
        .set_project_orbit_module_enabled(project.id, OrbitModuleId::Custom(module.id), true)
        .unwrap();

    let invocation = store
        .create_orbit_invocation(agent.id, project.id, module.id)
        .unwrap();
    let snapshot = store
        .read_orbit_invocation(invocation.id, agent.id, project.id)
        .unwrap();
    let values = BTreeMap::from([
        ("name".into(), serde_json::json!("signup_completed")),
        (
            "what_it_does".into(),
            serde_json::json!("Fires after account creation succeeds."),
        ),
        (
            "properties".into(),
            serde_json::json!(["plan — selected plan"]),
        ),
        ("notes".into(), serde_json::json!("")),
    ]);
    let result = store
        .apply_orbit_invocation_changes(
            invocation.id,
            agent.id,
            project.id,
            snapshot.data_revision,
            vec![OrbitRecordInput {
                id: None,
                section: Some("Onboarding".into()),
                values,
            }],
            Vec::new(),
        )
        .unwrap();
    assert_eq!(result.inserted, 1);
    let update = store
        .load_orbit_invocation_update(invocation.id, agent.id)
        .unwrap()
        .unwrap();
    assert_eq!(update.module_name, "Analytics");
    assert_eq!((update.inserted, update.updated, update.deleted), (1, 0, 0));
    assert!(!update.undone);
    assert_eq!(
        store
            .load_orbit_records(project.id, module.id)
            .unwrap()
            .len(),
        1
    );

    let stale = store.apply_orbit_invocation_changes(
        invocation.id,
        agent.id,
        project.id,
        snapshot.data_revision,
        vec![OrbitRecordInput {
            id: None,
            section: Some("Onboarding".into()),
            values: BTreeMap::from([
                ("name".into(), serde_json::json!("stale")),
                ("what_it_does".into(), serde_json::json!("Stale write")),
                ("properties".into(), serde_json::json!([])),
                ("notes".into(), serde_json::json!("")),
            ]),
        }],
        Vec::new(),
    );
    assert!(stale
        .unwrap_err()
        .to_string()
        .contains("call orbit_read again"));

    store
        .undo_orbit_invocation(invocation.id, agent.id)
        .unwrap();
    assert!(
        store
            .load_orbit_invocation_update(invocation.id, agent.id)
            .unwrap()
            .unwrap()
            .undone
    );
    assert!(store
        .load_orbit_records(project.id, module.id)
        .unwrap()
        .is_empty());
}

#[test]
fn orbit_field_archiving_preserves_existing_record_data() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let project = Project::from_path(PathBuf::from("/tmp/choro-orbit-schema"));
    let mut config = AppConfig::default();
    config.projects = vec![project.clone()];
    store.save_workspace_config(&config).unwrap();
    let mut module = store
        .save_orbit_module(&analytics_orbit_template())
        .unwrap();
    store
        .set_project_orbit_module_enabled(project.id, OrbitModuleId::Custom(module.id), true)
        .unwrap();
    store
        .save_orbit_record(
            project.id,
            module.id,
            OrbitRecordInput {
                id: None,
                section: Some("Onboarding".into()),
                values: BTreeMap::from([
                    ("name".into(), serde_json::json!("signup_started")),
                    ("what_it_does".into(), serde_json::json!("Starts signup")),
                    ("properties".into(), serde_json::json!([])),
                    ("notes".into(), serde_json::json!("Legacy note")),
                ]),
            },
        )
        .unwrap();

    module.fields.retain(|field| field.key != "notes");
    store.save_orbit_module(&module).unwrap();

    let record = store
        .load_orbit_records(project.id, module.id)
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(record.values["notes"], serde_json::json!("Legacy note"));
    store
        .save_orbit_record(
            project.id,
            module.id,
            OrbitRecordInput {
                id: Some(record.id),
                section: record.section,
                values: BTreeMap::from([
                    ("name".into(), serde_json::json!("signup_started")),
                    ("what_it_does".into(), serde_json::json!("Begins signup")),
                    ("properties".into(), serde_json::json!([])),
                ]),
            },
        )
        .unwrap();
    let edited = store
        .load_orbit_records(project.id, module.id)
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(edited.values["notes"], serde_json::json!("Legacy note"));
}

#[test]
fn orbit_archived_field_key_is_reactivated_instead_of_duplicated() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let project = Project::from_path(PathBuf::from("/tmp/choro-orbit-field-restore"));
    let mut config = AppConfig::default();
    config.projects = vec![project.clone()];
    store.save_workspace_config(&config).unwrap();
    let mut module = store
        .save_orbit_module(&analytics_orbit_template())
        .unwrap();
    let original_notes_id = module
        .fields
        .iter()
        .find(|field| field.key == "notes")
        .unwrap()
        .id;

    module.fields.retain(|field| field.key != "notes");
    module = store.save_orbit_module(&module).unwrap();
    module.fields.retain(|field| !field.archived);
    module.fields.push(OrbitFieldDefinition {
        id: Uuid::new_v4(),
        key: "notes".into(),
        label: "Notes".into(),
        kind: OrbitFieldKind::LongText,
        primary: false,
        sort_order: 3,
        archived: false,
    });

    let restored = store.save_orbit_module(&module).unwrap();
    let notes = restored
        .fields
        .iter()
        .find(|field| field.key == "notes" && !field.archived)
        .unwrap();
    assert_eq!(notes.id, original_notes_id);
}

#[test]
fn orbit_module_archive_and_revisions_preserve_bindings_and_records() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let project = Project::from_path(PathBuf::from("/tmp/choro-orbit-archive"));
    let mut config = AppConfig::default();
    config.projects = vec![project.clone()];
    store.save_workspace_config(&config).unwrap();
    let mut module = store
        .save_orbit_module(&analytics_orbit_template())
        .unwrap();
    assert_eq!(module.revision, 1);
    module.description.push_str(" Updated.");
    module = store.save_orbit_module(&module).unwrap();
    assert_eq!(module.revision, 2);
    store
        .set_project_orbit_module_enabled(project.id, OrbitModuleId::Custom(module.id), true)
        .unwrap();
    store
        .save_orbit_record(
            project.id,
            module.id,
            OrbitRecordInput {
                id: None,
                section: Some("Onboarding".into()),
                values: analytics_values("signup_started", "Starts signup"),
            },
        )
        .unwrap();
    let data_revision = store
        .load_project_orbit_bindings(project.id)
        .unwrap()
        .into_iter()
        .find(|binding| binding.module == OrbitModuleId::Custom(module.id))
        .unwrap()
        .data_revision;
    assert_eq!(data_revision, 1);

    store.set_orbit_module_archived(module.id, true).unwrap();
    let archived = store.load_orbit_module(module.id).unwrap().unwrap();
    assert!(archived.archived);
    assert_eq!(archived.revision, 3);
    assert_eq!(
        store
            .load_orbit_records(project.id, module.id)
            .unwrap()
            .len(),
        1
    );
    assert!(store
        .load_project_orbit_bindings(project.id)
        .unwrap()
        .into_iter()
        .any(|binding| binding.module == OrbitModuleId::Custom(module.id) && binding.enabled));

    store.set_orbit_module_archived(module.id, false).unwrap();
    let restored = store.load_orbit_module(module.id).unwrap().unwrap();
    assert!(!restored.archived);
    assert_eq!(restored.revision, 4);
    assert_eq!(
        store
            .load_orbit_records(project.id, module.id)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn orbit_schema_changes_reject_invalid_types_and_identity_collisions() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let project = Project::from_path(PathBuf::from("/tmp/choro-orbit-schema-validation"));
    let mut config = AppConfig::default();
    config.projects = vec![project.clone()];
    store.save_workspace_config(&config).unwrap();
    let module = store
        .save_orbit_module(&analytics_orbit_template())
        .unwrap();
    store
        .set_project_orbit_module_enabled(project.id, OrbitModuleId::Custom(module.id), true)
        .unwrap();
    for name in ["signup_started", "signup_completed"] {
        store
            .save_orbit_record(
                project.id,
                module.id,
                OrbitRecordInput {
                    id: None,
                    section: Some("Onboarding".into()),
                    values: analytics_values(name, "Same description"),
                },
            )
            .unwrap();
    }

    let mut invalid_type = module.clone();
    invalid_type
        .fields
        .iter_mut()
        .find(|field| field.key == "properties")
        .unwrap()
        .kind = OrbitFieldKind::ShortText;
    assert!(store
        .save_orbit_module(&invalid_type)
        .unwrap_err()
        .to_string()
        .contains("properties must be text"));

    let mut collision = module.clone();
    for field in &mut collision.fields {
        field.primary = field.key == "what_it_does";
        if field.primary {
            field.kind = OrbitFieldKind::ShortText;
        }
    }
    assert!(store
        .save_orbit_module(&collision)
        .unwrap_err()
        .to_string()
        .contains("same identity"));
    assert_eq!(
        store
            .load_orbit_module(module.id)
            .unwrap()
            .unwrap()
            .revision,
        1
    );
}

#[test]
fn orbit_multi_batch_undo_reverses_touched_records_and_preserves_interleaved_edits() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let project = Project::from_path(PathBuf::from("/tmp/choro-orbit-reverse-undo"));
    let agent = sample_agent(&project);
    let mut config = AppConfig::default();
    config.projects = vec![project.clone()];
    store.save_workspace_config(&config).unwrap();
    store.save_agents(std::slice::from_ref(&agent)).unwrap();
    let module = store
        .save_orbit_module(&analytics_orbit_template())
        .unwrap();
    store
        .set_project_orbit_module_enabled(project.id, OrbitModuleId::Custom(module.id), true)
        .unwrap();
    let invocation = store
        .create_orbit_invocation(agent.id, project.id, module.id)
        .unwrap();
    let first = store
        .apply_orbit_invocation_changes(
            invocation.id,
            agent.id,
            project.id,
            0,
            vec![OrbitRecordInput {
                id: None,
                section: Some("Onboarding".into()),
                values: analytics_values("agent_event", "First agent version"),
            }],
            Vec::new(),
        )
        .unwrap();
    let agent_record = store
        .load_orbit_records(project.id, module.id)
        .unwrap()
        .pop()
        .unwrap();
    store
        .save_orbit_record(
            project.id,
            module.id,
            OrbitRecordInput {
                id: None,
                section: Some("Checkout".into()),
                values: analytics_values("manual_event", "Manual record"),
            },
        )
        .unwrap();
    let snapshot = store
        .read_orbit_invocation(invocation.id, agent.id, project.id)
        .unwrap();
    assert_eq!(snapshot.data_revision, first.data_revision + 1);
    store
        .apply_orbit_invocation_changes(
            invocation.id,
            agent.id,
            project.id,
            snapshot.data_revision,
            vec![OrbitRecordInput {
                id: Some(agent_record.id),
                section: Some("Onboarding".into()),
                values: analytics_values("agent_event", "Second agent version"),
            }],
            Vec::new(),
        )
        .unwrap();

    store
        .undo_orbit_invocation(invocation.id, agent.id)
        .unwrap();
    let records = store.load_orbit_records(project.id, module.id).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].values["name"], serde_json::json!("manual_event"));
}

#[test]
fn orbit_multi_batch_undo_aborts_atomically_when_a_touched_record_was_interleaved() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let project = Project::from_path(PathBuf::from("/tmp/choro-orbit-conflict-undo"));
    let agent = sample_agent(&project);
    let mut config = AppConfig::default();
    config.projects = vec![project.clone()];
    store.save_workspace_config(&config).unwrap();
    store.save_agents(std::slice::from_ref(&agent)).unwrap();
    let module = store
        .save_orbit_module(&analytics_orbit_template())
        .unwrap();
    store
        .set_project_orbit_module_enabled(project.id, OrbitModuleId::Custom(module.id), true)
        .unwrap();
    let invocation = store
        .create_orbit_invocation(agent.id, project.id, module.id)
        .unwrap();
    store
        .apply_orbit_invocation_changes(
            invocation.id,
            agent.id,
            project.id,
            0,
            vec![OrbitRecordInput {
                id: None,
                section: Some("Onboarding".into()),
                values: analytics_values("agent_event", "Agent version"),
            }],
            Vec::new(),
        )
        .unwrap();
    let touched = store
        .load_orbit_records(project.id, module.id)
        .unwrap()
        .pop()
        .unwrap();
    store
        .save_orbit_record(
            project.id,
            module.id,
            OrbitRecordInput {
                id: Some(touched.id),
                section: touched.section.clone(),
                values: analytics_values("agent_event", "Manual correction"),
            },
        )
        .unwrap();
    let snapshot = store
        .read_orbit_invocation(invocation.id, agent.id, project.id)
        .unwrap();
    store
        .apply_orbit_invocation_changes(
            invocation.id,
            agent.id,
            project.id,
            snapshot.data_revision,
            vec![OrbitRecordInput {
                id: None,
                section: Some("Checkout".into()),
                values: analytics_values("second_agent_event", "Second batch"),
            }],
            Vec::new(),
        )
        .unwrap();

    assert!(store
        .undo_orbit_invocation(invocation.id, agent.id)
        .unwrap_err()
        .to_string()
        .contains("cannot be undone safely"));
    let records = store.load_orbit_records(project.id, module.id).unwrap();
    assert_eq!(records.len(), 2);
    assert!(records
        .iter()
        .any(|record| { record.values["what_it_does"] == serde_json::json!("Manual correction") }));
    assert!(records
        .iter()
        .any(|record| { record.values["name"] == serde_json::json!("second_agent_event") }));
}

fn analytics_values(name: &str, description: &str) -> BTreeMap<String, serde_json::Value> {
    BTreeMap::from([
        ("name".into(), serde_json::json!(name)),
        ("what_it_does".into(), serde_json::json!(description)),
        ("properties".into(), serde_json::json!([])),
        ("notes".into(), serde_json::json!("")),
    ])
}

#[test]
fn migrates_populated_v32_store_through_current_schema_without_touching_existing_data() {
    let dir = tempfile::tempdir().unwrap();
    let project = Project::from_path(PathBuf::from("/tmp/choro-orbit-v32"));
    {
        let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
        let mut config = AppConfig::default();
        config.projects = vec![project.clone()];
        store.save_workspace_config(&config).unwrap();
        store
            .rt
            .block_on(async {
                let conn = store.connect().await?;
                for table in [
                    "orbit_mutation_batches",
                    "orbit_invocations",
                    "orbit_records",
                    "orbit_project_modules",
                    "orbit_module_fields",
                    "orbit_modules",
                ] {
                    conn.execute(format!("DROP TABLE {table}"), ()).await?;
                }
                conn.execute("DELETE FROM schema_migrations WHERE version >= 33", ())
                    .await?;
                assert_eq!(schema_version(&conn).await?, 32);
                Ok::<_, anyhow::Error>(())
            })
            .unwrap();
    }

    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    assert_eq!(
        store
            .load_workspace_config(AppConfig::default())
            .unwrap()
            .projects,
        vec![project]
    );
    assert_eq!(store.load_orbit_modules(false).unwrap().len(), 0);
    store
        .rt
        .block_on(async {
            let conn = store.connect().await?;
            assert_eq!(schema_version(&conn).await?, STORE_SCHEMA_VERSION);
            Ok::<_, anyhow::Error>(())
        })
        .unwrap();
}

#[test]
fn current_schema_repairs_missing_orbit_tables() {
    let dir = tempfile::tempdir().unwrap();
    {
        let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
        store
            .rt
            .block_on(async {
                let conn = store.connect().await?;
                conn.execute("DROP TABLE orbit_mutation_batches", ())
                    .await?;
                conn.execute("DROP TABLE orbit_invocations", ()).await?;
                conn.execute("DROP TABLE orbit_records", ()).await?;
                conn.execute("DROP TABLE orbit_project_modules", ()).await?;
                conn.execute("DROP TABLE orbit_module_fields", ()).await?;
                conn.execute("DROP TABLE orbit_modules", ()).await?;
                assert_eq!(schema_version(&conn).await?, STORE_SCHEMA_VERSION);
                Ok::<_, anyhow::Error>(())
            })
            .unwrap();
    }
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let saved = store
        .save_orbit_module(&analytics_orbit_template())
        .unwrap();
    assert_eq!(saved.name, "Analytics");
}
