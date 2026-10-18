use super::*;
use crate::{AgentAccessMode, AgentEffort, AgentKind, AgentModel};

fn fixture() -> (
    tempfile::TempDir,
    LocalStore,
    Project,
    AgentRecord,
    AgentRecord,
) {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let project = Project::from_path(dir.path().join("project"));
    let mut config = AppConfig::default();
    config.projects.push(project.clone());
    store.save_workspace_config(&config).unwrap();
    let agent = AgentRecord::new(
        project.id,
        project.path.clone(),
        "Visual",
        "Test",
        AgentKind::Codex,
        AgentModel::CodexDefault,
        AgentEffort::Medium,
        AgentAccessMode::FullAccess,
    );
    let other = AgentRecord {
        id: Uuid::new_v4(),
        ..agent.clone()
    };
    store.save_agents(&[agent.clone(), other.clone()]).unwrap();
    (dir, store, project, agent, other)
}

fn input(key: &str) -> VisualPresentInput {
    VisualPresentInput {
        operation_key: key.into(),
        id: None,
        expected_revision: None,
        document: VisualDocument {
            title: "Button directions".into(),
            summary: "Two sample button styles.".into(),
            kind: VisualKind::DesignOptions,
            openui_lang: "root = Answer([choices])\nchoices = DesignOptions()".into(),
            datasets: Default::default(),
            sources: vec![],
            options: ["outline", "tonal"]
                .into_iter()
                .map(|id| VisualOption {
                    id: id.into(),
                    title: id.into(),
                    description: format!("The {id} direction"),
                    html: format!("<button>{id}</button>"),
                })
                .collect(),
            actions: vec![
                VisualAction {
                    id: "choose".into(),
                    label: "Use this style".into(),
                    kind: VisualActionKind::Select,
                },
                VisualAction {
                    id: "refine".into(),
                    label: "Refine".into(),
                    kind: VisualActionKind::Refine,
                },
            ],
        },
    }
}

#[test]
fn heads_follow_latest_publication_without_reordering_for_interactions_or_retries() {
    let (dir, store, project, agent, other) = fixture();
    let first = store
        .present_visual_answer(project.id.0, agent.id, input("first"))
        .unwrap()
        .answer;
    let second = store
        .present_visual_answer(project.id.0, agent.id, input("second"))
        .unwrap()
        .answer;
    store
        .present_visual_answer(project.id.0, other.id, input("other-chat"))
        .unwrap();
    let stamp = store.visual_answer_stamp(project.id.0, agent.id).unwrap();
    let ids = |heads: Vec<VisualAnswerHead>| {
        heads
            .into_iter()
            .map(|h| (h.id, h.revision))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        ids(store.visual_answer_heads(project.id.0, agent.id).unwrap()),
        vec![(first.id, 1), (second.id, 1)]
    );

    let mut update = input("revise-first");
    update.id = Some(first.id);
    update.expected_revision = Some(1);
    update.document.title = "Revised older answer".into();
    let revised = store
        .present_visual_answer(project.id.0, agent.id, update.clone())
        .unwrap()
        .answer;
    let revised_stamp = store.visual_answer_stamp(project.id.0, agent.id).unwrap();
    assert_ne!(stamp, revised_stamp);
    let expected = vec![(second.id, 1), (first.id, 2)];
    assert_eq!(
        ids(store.visual_answer_heads(project.id.0, agent.id).unwrap()),
        expected
    );

    // Neither using another answer nor retrying an earlier publication should
    // make it look newly published to the composer.
    let state = VisualInteraction {
        draft: "A note on the second answer".into(),
        ..second.interaction
    };
    store
        .save_visual_interaction(project.id.0, agent.id, second.id, 1, state)
        .unwrap();
    assert!(
        store
            .present_visual_answer(project.id.0, agent.id, input("second"))
            .unwrap()
            .duplicate
    );
    assert!(
        store
            .present_visual_answer(project.id.0, agent.id, update)
            .unwrap()
            .duplicate
    );
    assert_eq!(
        store.visual_answer_stamp(project.id.0, agent.id).unwrap(),
        revised_stamp
    );
    assert_eq!(
        ids(store.visual_answer_heads(project.id.0, agent.id).unwrap()),
        expected
    );
    assert_eq!(
        store
            .read_visual_answer(project.id.0, agent.id, first.id, Some(1))
            .unwrap()
            .document
            .title,
        "Button directions"
    );
    drop(store);
    let reopened = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let heads = reopened
        .visual_answer_heads(project.id.0, agent.id)
        .unwrap();
    assert_eq!(heads.last().unwrap().created_at, first.created_at);
    assert_eq!(ids(heads), expected);
    assert_eq!(revised.revision, 2);
}

#[test]
fn publication_retries_and_revisions_are_durable_and_scoped() {
    let (dir, store, project, agent, other) = fixture();
    let first = store
        .present_visual_answer(project.id.0, agent.id, input("create"))
        .unwrap();
    assert!(!first.duplicate);
    let replay = store
        .present_visual_answer(project.id.0, agent.id, input("create"))
        .unwrap();
    assert!(replay.duplicate);
    assert_eq!(first.answer.id, replay.answer.id);
    let mut collision = input("create");
    collision.document.title = "Changed".into();
    assert!(store
        .present_visual_answer(project.id.0, agent.id, collision)
        .is_err());
    assert!(store
        .read_visual_answer(project.id.0, other.id, first.answer.id, None)
        .is_err());
    assert!(store
        .read_visual_answer(Uuid::new_v4(), agent.id, first.answer.id, None)
        .is_err());
    let mut update = input("update");
    update.id = Some(first.answer.id);
    update.expected_revision = Some(1);
    update.document.title = "Softer buttons".into();
    assert_eq!(
        store
            .present_visual_answer(project.id.0, agent.id, update.clone())
            .unwrap()
            .answer
            .revision,
        2
    );
    update.operation_key = "stale-update".into();
    assert!(store
        .present_visual_answer(project.id.0, agent.id, update)
        .unwrap_err()
        .to_string()
        .contains("revision conflict"));
    drop(store);
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    assert_eq!(
        store
            .read_visual_answer(project.id.0, agent.id, first.answer.id, Some(1))
            .unwrap()
            .document
            .title,
        "Button directions"
    );
    assert_eq!(
        store.list_visual_answers(project.id.0, agent.id).unwrap()[0].revision,
        2
    );
    assert!(
        store.load_project_previews(project.id).unwrap().is_empty(),
        "Publication never creates a Preview"
    );
}

#[test]
fn selection_draft_and_uncertain_submission_survive_restart_without_resending() {
    let (dir, store, project, agent, _) = fixture();
    let answer = store
        .present_visual_answer(project.id.0, agent.id, input("create"))
        .unwrap()
        .answer;
    assert!(store
        .begin_visual_submission(project.id.0, agent.id, answer.id, 1, 0, "choose")
        .is_err());
    let state = VisualInteraction {
        selected_option_id: Some("tonal".into()),
        draft: "Rounder corners".into(),
        fields: serde_json::json!({"search":"round"}),
        scroll_top: 93.0,
        ..Default::default()
    };
    let saved = store
        .save_visual_interaction(project.id.0, agent.id, answer.id, 1, state.clone())
        .unwrap();
    assert_eq!(saved.interaction.revision, 1);
    assert!(store
        .save_visual_interaction(project.id.0, agent.id, answer.id, 1, state)
        .is_err());
    let pending = store
        .begin_visual_submission(project.id.0, agent.id, answer.id, 1, 1, "choose")
        .unwrap();
    let token = pending.interaction.submission.as_ref().unwrap().id;
    assert!(pending
        .interaction
        .submission
        .as_ref()
        .unwrap()
        .text
        .contains("Rounder corners"));
    assert!(store
        .begin_visual_submission(project.id.0, agent.id, answer.id, 1, 2, "choose")
        .is_err());
    drop(store);
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let restored = store
        .read_visual_answer(project.id.0, agent.id, answer.id, None)
        .unwrap();
    assert_eq!(
        restored.interaction.selected_option_id.as_deref(),
        Some("tonal")
    );
    assert_eq!(restored.interaction.fields["search"], "round");
    assert_eq!(
        restored.interaction.submission.as_ref().unwrap().status,
        VisualSubmissionStatus::Sending
    );
    assert!(store
        .finish_visual_submission(project.id.0, agent.id, answer.id, 1, Uuid::new_v4(), true)
        .is_err());
    let receipt = store
        .finish_visual_submission(project.id.0, agent.id, answer.id, 1, token, true)
        .unwrap();
    assert_eq!(
        receipt.interaction.submission.unwrap().status,
        VisualSubmissionStatus::Sent
    );
    assert!(receipt.interaction.collapsed);
    assert!(store
        .begin_visual_submission(
            project.id.0,
            agent.id,
            answer.id,
            1,
            receipt.interaction.revision,
            "choose"
        )
        .is_err());
}

#[test]
fn failed_dispatch_is_retryable_but_stale_renderer_state_cannot_overwrite_it() {
    let (_dir, store, project, agent, _) = fixture();
    let answer = store
        .present_visual_answer(project.id.0, agent.id, input("create"))
        .unwrap()
        .answer;
    let state = VisualInteraction {
        selected_option_id: Some("tonal".into()),
        ..Default::default()
    };
    let saved = store
        .save_visual_interaction(project.id.0, agent.id, answer.id, 1, state)
        .unwrap();
    let pending = store
        .begin_visual_submission(
            project.id.0,
            agent.id,
            answer.id,
            1,
            saved.interaction.revision,
            "choose",
        )
        .unwrap();
    let failed = store
        .finish_visual_submission(
            project.id.0,
            agent.id,
            answer.id,
            1,
            pending.interaction.submission.unwrap().id,
            false,
        )
        .unwrap();
    assert!(failed.interaction.submission.is_none());
    assert!(failed.interaction.submission_error.is_some());
    assert!(store
        .save_visual_interaction(project.id.0, agent.id, answer.id, 1, saved.interaction)
        .is_err());
    store
        .begin_visual_submission(
            project.id.0,
            agent.id,
            answer.id,
            1,
            failed.interaction.revision,
            "choose",
        )
        .unwrap();
}

#[test]
fn stale_answer_updates_and_forged_submission_state_are_rejected() {
    let (_dir, store, project, agent, _) = fixture();
    let answer = store
        .present_visual_answer(project.id.0, agent.id, input("create"))
        .unwrap()
        .answer;
    let forged = VisualInteraction {
        submission: Some(VisualSubmission {
            id: Uuid::new_v4(),
            action_id: "choose".into(),
            text: "forged".into(),
            status: VisualSubmissionStatus::Sent,
        }),
        ..Default::default()
    };
    assert!(store
        .save_visual_interaction(project.id.0, agent.id, answer.id, 1, forged)
        .is_err());
    let mut update = input("update");
    update.id = Some(answer.id);
    update.expected_revision = Some(1);
    store
        .present_visual_answer(project.id.0, agent.id, update)
        .unwrap();
    assert!(store
        .save_visual_interaction(
            project.id.0,
            agent.id,
            answer.id,
            1,
            VisualInteraction::default()
        )
        .is_err());
    assert!(store
        .begin_visual_submission(project.id.0, agent.id, answer.id, 1, 0, "choose")
        .is_err());
}

#[test]
fn concurrent_publication_and_submission_reserve_a_single_winner() {
    let (_dir, store, project, agent, _) = fixture();
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let store = store.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store.present_visual_answer(project.id.0, agent.id, input("concurrent"))
            })
        })
        .collect();
    let results: Vec<_> = handles
        .into_iter()
        .map(|h| h.join().unwrap().unwrap())
        .collect();
    assert_eq!(results[0].answer.id, results[1].answer.id);
    assert_ne!(results[0].duplicate, results[1].duplicate);
    let id = results[0].answer.id;
    let saved = store
        .save_visual_interaction(
            project.id.0,
            agent.id,
            id,
            1,
            VisualInteraction {
                selected_option_id: Some("outline".into()),
                ..Default::default()
            },
        )
        .unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let store = store.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store.begin_visual_submission(
                    project.id.0,
                    agent.id,
                    id,
                    1,
                    saved.interaction.revision,
                    "choose",
                )
            })
        })
        .collect();
    assert_eq!(
        handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .filter(Result::is_ok)
            .count(),
        1
    );
}

fn table_input(key: &str, title: &str) -> VisualPresentInput {
    let mut input = input(key);
    input.document = serde_json::from_value(serde_json::json!({
        "title": title, "summary": "Sample projects", "kind": "table",
        "openui_lang": "root = Answer([projects])\nprojects = DataTable(\"projects\")",
        "datasets": {"projects": {"columns": [
            {"key":"name","label":"Project","kind":"text"},
            {"key":"open","label":"Open issues","kind":"number"}],
            "rows": [{"name":"Atlas","open":4}]}},
        "sources": [{"label":"Sample data"}]
    }))
    .unwrap();
    input
}

#[test]
fn compatible_revisions_keep_filters_and_submitted_answers_stay_inspectable() {
    let (_dir, store, project, agent, _) = fixture();
    let answer = store
        .present_visual_answer(project.id.0, agent.id, table_input("create", "Projects"))
        .unwrap()
        .answer;
    let saved = store
        .save_visual_interaction(
            project.id.0,
            agent.id,
            answer.id,
            1,
            VisualInteraction {
                fields: serde_json::json!({"view":{"s:projects":{"search":"atl"}},"summary":"Table projects: search \"atl\"."}),
                scroll_top: 40.0,
                ..Default::default()
            },
        )
        .unwrap();
    let pending = store
        .begin_visual_submission_with_draft(
            project.id.0,
            agent.id,
            answer.id,
            1,
            saved.interaction.revision,
            IMPLICIT_REFINE_ACTION,
            Some("Only the blocked projects".into()),
        )
        .unwrap();
    let receipt = pending.interaction.submission.clone().unwrap();
    assert!(receipt.text.contains("Only the blocked projects"));
    assert!(receipt.text.contains("search \"atl\""));
    let sent = store
        .finish_visual_submission(project.id.0, agent.id, answer.id, 1, receipt.id, true)
        .unwrap();
    assert!(sent.interaction.draft.is_empty());
    // Reopening and filtering a submitted revision still persists.
    let reopened = store
        .save_visual_interaction(
            project.id.0,
            agent.id,
            answer.id,
            1,
            VisualInteraction {
                collapsed: false,
                fields: serde_json::json!({"view":{"s:projects":{"search":""}}}),
                ..sent.interaction.clone()
            },
        )
        .unwrap();
    assert!(!reopened.interaction.collapsed);
    // Its decision cannot change.
    assert!(store
        .save_visual_interaction(
            project.id.0,
            agent.id,
            answer.id,
            1,
            VisualInteraction {
                draft: "changed".into(),
                ..reopened.interaction.clone()
            },
        )
        .is_err());

    let mut update = table_input("retitle", "Projects by owner");
    update.id = Some(answer.id);
    update.expected_revision = Some(1);
    let revised = store
        .present_visual_answer(project.id.0, agent.id, update)
        .unwrap()
        .answer;
    assert_eq!(
        revised.interaction.fields["view"]["s:projects"]["search"],
        ""
    );
    assert_eq!(
        revised.interaction.scroll_top, 40.0,
        "an unchanged program keeps its scroll position"
    );
    assert!(revised.interaction.submission.is_none());

    let mut incompatible = table_input("reshape", "Projects");
    incompatible.id = Some(answer.id);
    incompatible.expected_revision = Some(2);
    incompatible
        .document
        .datasets
        .get_mut("projects")
        .unwrap()
        .columns[1]
        .key = "issues".into();
    incompatible
        .document
        .datasets
        .get_mut("projects")
        .unwrap()
        .rows = vec![std::collections::BTreeMap::from([
        ("name".into(), serde_json::json!("Atlas")),
        ("issues".into(), serde_json::json!(4)),
    ])];
    let reshaped = store
        .present_visual_answer(project.id.0, agent.id, incompatible)
        .unwrap()
        .answer;
    assert_eq!(reshaped.interaction.fields, serde_json::json!({}));

    let heads = store.visual_answer_heads(project.id.0, agent.id).unwrap();
    assert_eq!(heads.len(), 1);
    assert_eq!((heads[0].id, heads[0].revision), (answer.id, 3));
}

#[test]
fn detaching_keeps_the_choice_across_restart_and_a_new_revision_reattaches() {
    let (dir, store, project, agent, _) = fixture();
    let answer = store
        .present_visual_answer(project.id.0, agent.id, input("detach"))
        .unwrap()
        .answer;
    let state = VisualInteraction {
        selected_option_id: Some("tonal".into()),
        draft: "Rounder corners".into(),
        collapsed: true,
        detached: true,
        ..answer.interaction.clone()
    };
    store
        .save_visual_interaction(project.id.0, agent.id, answer.id, 1, state)
        .unwrap();
    drop(store);
    let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
    let restored = store
        .read_visual_answer(project.id.0, agent.id, answer.id, None)
        .unwrap();
    assert!(restored.interaction.detached && restored.interaction.collapsed);
    assert_eq!(restored.interaction.selected_option_id.as_deref(), Some("tonal"));
    assert_eq!(restored.interaction.draft, "Rounder corners");

    // The agent's explicit revision is a new question: it starts attached,
    // carrying the note and a still-valid selection.
    let mut update = input("detach-2");
    update.id = Some(answer.id);
    update.expected_revision = Some(1);
    let revised = store
        .present_visual_answer(project.id.0, agent.id, update)
        .unwrap()
        .answer;
    assert!(!revised.interaction.detached);
    assert_eq!(revised.interaction.draft, "Rounder corners");

    // State saved before detach existed still loads.
    let legacy: VisualInteraction = serde_json::from_value(serde_json::json!({
        "revision": 2, "selected_option_id": null, "draft": "", "fields": {},
        "scroll_top": 0.0, "collapsed": false, "submission": null, "submission_error": null
    }))
    .unwrap();
    assert!(!legacy.detached);
}
