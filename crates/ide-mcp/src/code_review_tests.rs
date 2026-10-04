use super::*;
use ide_core::agent_changes::{ChangeKey, EvidenceKind, MutationEvidence};
use std::sync::atomic::AtomicBool;

fn setup() -> (tempfile::TempDir, ServerContext, ReviewRun) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("repo");
    std::fs::create_dir(&root).unwrap();
    git2::Repository::init(&root).unwrap();
    std::fs::write(root.join("test.rs"), "new\n").unwrap();
    let store = LocalStore::open(dir.path().join("store")).unwrap();
    let mut run = ReviewRun::new(
        uuid::Uuid::new_v4(),
        uuid::Uuid::new_v4(),
        "Claude".into(),
        "model".into(),
        "effort".into(),
        review_now(),
    );
    store.create_review_run(&run).unwrap();
    let evidence = MutationEvidence {
        key: ChangeKey {
            project_id: run.project_id,
            root: root.clone(),
            agent_id: run.parent_id,
            generation: "generation".into(),
            turn_id: "turn".into(),
            action_id: "edit".into(),
            path: "test.rs".into(),
        },
        kind: EvidenceKind::Contents,
        confirmed: true,
        additions: None,
        deletions: None,
        before_hash: None,
        after_hash: None,
        before: Some("old\n".into()),
        after: Some("new\n".into()),
        patch: None,
        captured_at: 1,
    };
    let storage = store.review_storage(run.id);
    let input = prepare_review(
        &mut run,
        &root,
        &storage,
        ReviewRequirements {
            user_requirements: vec!["Fix cancellation".into()],
            decisions: vec![],
            checks: vec![],
            project_rules: vec![],
            supplementary_guidance: String::new(),
        },
        &[evidence],
        &[],
        &AtomicBool::new(false),
    )
    .unwrap();
    store.save_review_input(&run, &input).unwrap();
    let prepared = run.clone();
    run = store
        .transact_review(run.id, Some(0), |r| {
            *r = prepared;
            Ok(())
        })
        .unwrap()
        .0;
    let ctx = ServerContext {
        review_run: Some(run.id),
        studio: false,
        delegation_scope: None,
        project_id: Some(run.project_id),
        agent_id: Some(run.reviewer_id),
        store: Some(store),
    };
    (dir, ctx, run)
}
fn call(ctx: &ServerContext, name: &str, args: Value) -> Value {
    ToolRegistry::default().call(ctx, &json!({"name":name,"arguments":args}))
}

#[test]
fn review_listing_and_dispatch_expose_exactly_the_bound_four_tools() {
    let (_dir, mut ctx, run) = setup();
    let registry = ToolRegistry::default();
    let names = registry
        .list_for(&ctx)
        .into_iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(names.len(), 4);
    assert!(names.iter().all(|n| allowed(n)));
    assert_eq!(call(&ctx, "review_context", json!({}))["isError"], false);
    for name in [
        "get_agent_changes",
        "memory_save",
        "task_read",
        "studio_read",
        "delegation_plan",
        "summary_read",
    ] {
        assert_eq!(call(&ctx, name, json!({}))["isError"], true, "{name}");
    }
    ctx.agent_id = Some(run.parent_id);
    assert!(registry.list_for(&ctx).is_empty());
    assert_eq!(call(&ctx, "review_context", json!({}))["isError"], true);
    ctx.agent_id = Some(run.reviewer_id);
    ctx.project_id = Some(uuid::Uuid::new_v4());
    assert!(registry.list_for(&ctx).is_empty());
    ctx.project_id = Some(run.project_id);
    ctx.review_run = Some(uuid::Uuid::new_v4());
    assert!(registry.list_for(&ctx).is_empty());
    ctx.review_run = None;
    assert!(!registry
        .list_for(&ctx)
        .iter()
        .any(|t| allowed(t["name"].as_str().unwrap())));
    assert_eq!(call(&ctx, "review_context", json!({}))["isError"], true);
}
#[test]
fn review_cannot_choose_foreign_run_live_path_or_invalid_page() {
    let (_dir, ctx, run) = setup();
    assert_eq!(
        call(
            &ctx,
            "review_context",
            json!({"run_id":uuid::Uuid::new_v4()})
        )["isError"],
        true
    );
    for path in ["../secret", "/etc/passwd", ".env", ".git/config"] {
        assert_eq!(
            call(
                &ctx,
                "review_read",
                json!({"kind":"source","path":path,"start":1,"end":1})
            )["isError"],
            true
        );
    }
    assert_eq!(
        call(
            &ctx,
            "review_read",
            json!({"kind":"diff","file_id":run.files[0].id,"page":999})
        )["isError"],
        true
    );
    assert_eq!(
        call(
            &ctx,
            "review_report",
            json!({"report":{"action":"file_complete","file_id":run.files[0].id}})
        )["isError"],
        true
    );
    assert_eq!(
        call(
            &ctx,
            "review_read",
            json!({"kind":"diff","file_id":run.files[0].id,"page":0})
        )["isError"],
        false
    );
    assert_eq!(
        call(
            &ctx,
            "review_report",
            json!({"report":{"action":"file_complete","file_id":run.files[0].id}})
        )["isError"],
        false
    );
    assert_eq!(
        call(
            &ctx,
            "review_report",
            json!({"report":{"action":"file_complete","file_id":run.files[0].id}})
        )["isError"],
        false
    );
    assert_eq!(
        ctx.store()
            .unwrap()
            .load_review_run(run.id)
            .unwrap()
            .completed_files(),
        1
    );
}
#[test]
fn cancellation_and_terminal_states_revoke_listing_reads_and_late_findings() {
    let (_dir, ctx, run) = setup();
    ctx.store()
        .unwrap()
        .transact_review(run.id, None, |r| {
            r.state = ReviewRunState::Cancelling;
            Ok(())
        })
        .unwrap();
    assert!(ToolRegistry::default().list_for(&ctx).is_empty());
    assert_eq!(
        call(
            &ctx,
            "review_read",
            json!({"kind":"source","path":"test.rs","start":1,"end":1})
        )["isError"],
        true
    );
    assert_eq!(
        call(
            &ctx,
            "review_report",
            json!({"report":{"action":"finalize"}})
        )["isError"],
        true
    );
    ctx.store()
        .unwrap()
        .transact_review(run.id, None, |r| {
            r.stop(ReviewRunState::Cancelled, None, review_now());
            Ok(())
        })
        .unwrap();
    assert_eq!(call(&ctx, "review_context", json!({}))["isError"], true);
}

#[test]
fn compact_sections_and_group_calls_keep_the_same_authorization_and_accounting() {
    let (_dir, mut ctx, run) = setup();
    for args in [json!({"section":"foreign"}), json!({"page":-1}),json!({"page":"0"}),json!({"section":"files","page":999})] {
        assert_eq!(call(&ctx,"review_context",args)["isError"],true);
    }
    assert_eq!(call(&ctx,"review_context",json!({"section":"files","page":0}))["isError"],false);
    assert_eq!(call(&ctx,"review_report",json!({"report":{"action":"files_complete","file_ids":[run.files[0].id]}}))["isError"],true);
    assert_eq!(call(&ctx,"review_read",json!({"kind":"batch","batch":999,"page":0}))["isError"],true);
    assert_eq!(call(&ctx,"review_read",json!({"kind":"batch","batch":0,"page":0}))["isError"],false);
    assert_eq!(call(&ctx,"review_report",json!({"report":{"action":"files_complete","file_ids":[run.files[0].id,"foreign"]}}))["isError"],true);
    assert_eq!(ctx.store().unwrap().load_review_run(run.id).unwrap().completed_files(),0);
    assert_eq!(call(&ctx,"review_report",json!({"report":{"action":"files_complete","file_ids":[run.files[0].id]}}))["isError"],false);
    ctx.agent_id = Some(run.parent_id);
    assert_eq!(call(&ctx,"review_read",json!({"kind":"batch","batch":0,"page":0}))["isError"],true);
    assert_eq!(call(&ctx,"review_context",json!({"section":"checks"}))["isError"],true);
}

#[test]
fn rejected_finalization_returns_pending_candidates_for_recovery() {
    let (_dir, ctx, run) = setup();
    let file = &run.files[0];
    let candidate = json!({"id":"pending-bug","severity":"Medium",
        "location":{"file_id":file.id,"path":file.path,"side":"After","start":1,"end":1},
        "title":"Changed value breaks the expected result","trigger":"The changed path is used",
        "consequence":"The caller receives the wrong value","suggested_fix":"Preserve the expected value",
        "evidence":[{"file_id":file.id,"path":file.path,"side":"After",
            "content_hash":file.after_hash,"start":1,"end":1,"excerpt":"new"}],"challenge":""});
    assert_eq!(call(&ctx, "review_report", json!({"report":{"action":"candidate","finding":candidate}}))["isError"], false);
    assert_eq!(call(&ctx, "review_report", json!({"report":{"action":"stage","stage":"CheckingFindings"}}))["isError"], false);
    assert_eq!(call(&ctx, "review_report", json!({"report":{"action":"finalize"}}))["isError"], true);
    let result = call(&ctx, "review_context", json!({}));
    assert_eq!(result["isError"], false);
    let context: Value = serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(context["pending_candidates"], json!([candidate]));
    let current = ctx.store().unwrap().load_review_run(run.id).unwrap();
    assert_eq!(current.state, ReviewRunState::Running);
    assert!(!current.finalized_by_reviewer);
}

#[test]
fn finalization_replay_acknowledges_the_same_result_without_accepting_late_reports() {
    let (_dir, ctx, run) = setup();
    call(
        &ctx,
        "review_read",
        json!({"kind":"diff","file_id":run.files[0].id,"page":0}),
    );
    call(
        &ctx,
        "review_report",
        json!({"report":{"action":"file_complete","file_id":run.files[0].id}}),
    );
    call(
        &ctx,
        "review_report",
        json!({"report":{"action":"stage","stage":"CheckingFindings"}}),
    );
    assert_eq!(
        call(
            &ctx,
            "review_report",
            json!({"report":{"action":"finalize"}})
        )["isError"],
        false
    );
    let revision = ctx
        .store()
        .unwrap()
        .load_review_run(run.id)
        .unwrap()
        .revision;
    assert_eq!(
        call(
            &ctx,
            "review_report",
            json!({"report":{"action":"finalize"}})
        )["isError"],
        false
    );
    assert_eq!(
        ctx.store()
            .unwrap()
            .load_review_run(run.id)
            .unwrap()
            .revision,
        revision
    );
    assert_eq!(
        call(
            &ctx,
            "review_report",
            json!({"report":{"action":"stage","stage":"CheckingFindings"}})
        )["isError"],
        true
    );
}
