use super::*;
use crate::agents::{
    AgentAccessMode, AgentEffort, AgentKind, AgentModel, AgentRuntimeKind, AgentStatus,
};
use crate::git::read::fixtures::{commit_all, repo_with_commit, workdir};
use crate::git::{DiffHunk, DiffLine};
use crate::task_tracker::{IssueTrackerProvider, TaskTrackerConnection};
use crate::DbProvider;
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
    assert_eq!(loaded_solo.solo_branch, solo.solo_branch);
    assert_eq!(loaded_solo.solo_base_branch, solo.solo_base_branch);
    assert_eq!(loaded_solo.solo_rejoined_branch, solo.solo_rejoined_branch);
    assert_eq!(loaded_solo.lane_profile, Some(LaneProfile::Full));
    assert!(loaded_solo.is_solo());

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
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let mut config = AppConfig::default();
    config.projects = vec![project];
    store.save_workspace_config(&config).unwrap();
    store.save_agents(&[agent.clone()]).unwrap();

    assert_eq!(store.load_agents().unwrap(), vec![agent]);
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
