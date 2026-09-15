use super::*;
use ide_core::{
    delegation::{DelegationAttempt, DelegationBinding},
    experts::ExpertProfile,
    local_store::LocalStore,
    AgentAccessMode, AgentKind, AgentModel, AgentRecord, AgentRuntimeKind, AppConfig, Project,
};

fn fixture() -> (tempfile::TempDir, ServerContext, Uuid, ExpertProfile) {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().join("store")).unwrap();
    store
        .save_beta_features(ide_core::config::BetaFeatures { delegation: true })
        .unwrap();
    let repo = dir.path().join("repo");
    git2::Repository::init(&repo).unwrap();
    let project = Project::from_path(repo);
    let mut config = AppConfig::default();
    config.projects.push(project.clone());
    store.save_workspace_config(&config).unwrap();
    let model = AgentModel::default_for(AgentKind::Codex);
    let mut parent = AgentRecord::new(
        project.id,
        project.path,
        "Lead",
        "Build",
        AgentKind::Codex,
        model,
        model.default_effort(),
        AgentAccessMode::default(),
    );
    parent.runtime = AgentRuntimeKind::Chat;
    store.save_agents(&[parent.clone()]).unwrap();
    let p = store
        .save_expert(
            ExpertProfile {
                id: Uuid::new_v4(),
                revision: 0,
                name: "Builder".into(),
                description: "Build".into(),
                provider: AgentKind::Codex,
                model,
                effort: model.default_effort(),
                instructions: "Build the assigned files".into(),
                skills: vec![],
                expected_outcome: "Checked".into(),
                enabled: true,
                archived: false,
                additions: Default::default(),
            },
            None,
        )
        .unwrap();
    let source = Uuid::new_v4();
    store
        .authorize_experts(parent.id, source, "Delegate to Builder", &[], false)
        .unwrap();
    let ctx = ServerContext {
        project_id: Some(project.id.0),
        agent_id: Some(parent.id),
        store: Some(store),
        delegation_scope: None,
    };
    (dir, ctx, source, p)
}
fn plan(ctx: &ServerContext, source: Uuid, p: &ExpertProfile) -> Value {
    let repo = ctx.store().unwrap().load_agents().unwrap()[0]
        .project_path
        .clone();
    json!({"authorization_id":source,"operation_key":"plan-1","expected_revision":0,"tasks":[{"key":"build","expert_id":p.id,"goal":"Build","brief":"Implement a file","expected_outcome":"Checked","repository":repo,"kind":"implementation","dependencies":[],"held":false}]})
}
fn content(value: Vec<Value>) -> Value {
    serde_json::from_str(value[0]["text"].as_str().unwrap()).unwrap()
}

#[test]
fn typo_role_request_survives_invalid_plan_and_schedules_exactly_its_team() {
    let (_dir, ctx, _, _) = fixture();
    let store = ctx.store().unwrap();
    store.ensure_default_experts().unwrap();
    let source = Uuid::new_v4();
    let request = "Please add a new landing page and delegate expets ui designder fro deisgn, ux writie for texrt and frotnend for the build the code of front";
    let grant = store
        .authorize_experts(ctx.agent_id().unwrap(), source, request, &[], false)
        .unwrap()
        .unwrap();
    let discovery = content(
        DelegationTool("experts_list")
            .call_enabled(&ctx, &json!({}))
            .unwrap(),
    );
    assert_eq!(discovery["authorization_status"], "ready");
    assert_eq!(
        discovery["authorized_expert_ids"].as_array().unwrap().len(),
        3
    );
    let profiles = store.load_experts().unwrap();
    let expert_id = |name: &str| profiles.iter().find(|p| p.name == name).unwrap().id;
    for name in ["UI Designer", "UX Writer", "Frontend Engineer"] {
        assert!(grant.expert_ids.contains(&expert_id(name)));
    }
    let tasks = vec![
        json!({"key":"design","expert_id":expert_id("UI Designer"),"goal":"Design a new landing page","brief":"Provide layout decisions without reading existing site files","expected_outcome":"Layout specification","kind":"consultation","dependencies":[]}),
        json!({"key":"copy","expert_id":expert_id("UX Writer"),"goal":"Write landing page copy","brief":"Provide new coffee shop text","expected_outcome":"Final copy","kind":"consultation","dependencies":[]}),
        json!({"key":"frontend","expert_id":expert_id("Frontend Engineer"),"goal":"Implement a standalone page","brief":"Build the approved layout and copy in a new HTML file","expected_outcome":"Working new page","kind":"implementation","dependencies":["design","copy"]}),
    ];
    let mut args = json!({"authorization_id":source,"operation_key":"role-request","expected_revision":0,"tasks":tasks});
    let error = DelegationTool("delegation_plan")
        .call_enabled(&ctx, &args)
        .unwrap_err()
        .to_string();
    assert!(error.contains("repository") && error.contains("Each task"));
    assert!(store.load_delegations().unwrap().is_empty());
    let still_authorized = store
        .latest_expert_authorization(ctx.agent_id().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::to_value(still_authorized).unwrap(),
        serde_json::to_value(&grant).unwrap()
    );
    for task in args["tasks"].as_array_mut().unwrap() {
        task["repository"] = discovery["working_directory"].clone();
    }
    let first = DelegationTool("delegation_plan")
        .call_enabled(&ctx, &args)
        .unwrap();
    assert_eq!(
        DelegationTool("delegation_plan")
            .call_enabled(&ctx, &args)
            .unwrap(),
        first
    );
    let runs = store.load_delegations().unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].tasks.len(), 3);
    assert_eq!(runs[0].tasks[2].plan.dependencies, ["design", "copy"]);
    assert_eq!(
        store.load_agents().unwrap().len(),
        1,
        "Planning must not spawn provider or standalone child records"
    );
}

#[test]
fn empty_authorization_discovery_requires_clarification_instead_of_a_plan_retry() {
    let (_dir, ctx, _, expert) = fixture();
    let source = Uuid::new_v4();
    ctx.store()
        .unwrap()
        .authorize_experts(
            ctx.agent_id().unwrap(),
            source,
            "Build a coffee page",
            &[],
            false,
        )
        .unwrap();
    let discovery = content(
        DelegationTool("experts_list")
            .call_enabled(&ctx, &json!({}))
            .unwrap(),
    );
    assert_eq!(discovery["authorization_status"], "needs_expert_selection");
    assert_eq!(discovery["authorized_expert_ids"], json!([]));
    assert!(discovery["guidance"]
        .as_str()
        .unwrap()
        .contains("Do not call delegation_plan"));
    let error = DelegationTool("delegation_plan")
        .call_enabled(&ctx, &plan(&ctx, source, &expert))
        .unwrap_err()
        .to_string();
    assert!(error.contains("confirm the exact names"));
    assert!(ctx.store().unwrap().load_delegations().unwrap().is_empty());
}

#[test]
fn explicit_picker_selection_resolves_ambiguous_typo_without_expanding_team() {
    let (_dir, ctx, _, template) = fixture();
    let store = ctx.store().unwrap();
    let mut first = template.clone();
    first.id = Uuid::new_v4();
    first.revision = 0;
    first.name = "UI Writer".into();
    let first = store.save_expert(first, None).unwrap();
    let mut second = template.clone();
    second.id = Uuid::new_v4();
    second.revision = 0;
    second.name = "UI Writter".into();
    store.save_expert(second, None).unwrap();
    let request = "Delegate to UI Writie";
    assert!(store
        .authorize_experts(ctx.agent_id().unwrap(), Uuid::new_v4(), request, &[], false)
        .is_err());
    let grant = store
        .authorize_experts(
            ctx.agent_id().unwrap(),
            Uuid::new_v4(),
            request,
            &[first.id],
            false,
        )
        .unwrap()
        .unwrap();
    assert_eq!(grant.expert_ids, [first.id]);
}

#[test]
fn beta_off_rejects_new_tool_assignments_and_explains_where_to_enable() {
    let (_dir, ctx, source, expert) = fixture();
    let store = ctx.store().unwrap();
    store
        .save_beta_features(ide_core::config::BetaFeatures::default())
        .unwrap();
    for (name, args) in [
        ("experts_list", json!({})),
        ("delegation_plan", plan(&ctx, source, &expert)),
    ] {
        let error = DelegationTool(name).call_enabled(&ctx, &args).unwrap_err();
        assert!(error
            .to_string()
            .contains("Settings → Beta features → Delegation"));
    }
    assert!(store.load_delegations().unwrap().is_empty());
    assert_eq!(store.load_agents().unwrap().len(), 1);
}

#[test]
fn beta_off_keeps_existing_team_discovery_read_and_idempotent_plan_available() {
    let (_dir, ctx, source, expert) = fixture();
    let args = plan(&ctx, source, &expert);
    let first = DelegationTool("delegation_plan")
        .call_enabled(&ctx, &args)
        .unwrap();
    let run_id = content(first.clone())["run_id"].clone();
    ctx.store()
        .unwrap()
        .save_beta_features(ide_core::config::BetaFeatures::default())
        .unwrap();
    let discovery = content(
        DelegationTool("experts_list")
            .call_enabled(&ctx, &json!({}))
            .unwrap(),
    );
    assert_eq!(discovery["active_run"]["id"], run_id);
    let read = content(
        DelegationTool("delegation_read")
            .call_enabled(&ctx, &json!({"run_id": run_id}))
            .unwrap(),
    );
    assert_eq!(read["id"], run_id);
    assert_eq!(
        DelegationTool("delegation_plan")
            .call_enabled(&ctx, &args)
            .unwrap(),
        first
    );
    assert_eq!(ctx.store().unwrap().load_agents().unwrap().len(), 1);
}

#[test]
fn plan_replay_survives_profile_disable_and_never_creates_gui_agent_rows() {
    let (_dir, ctx, source, p) = fixture();
    let args = plan(&ctx, source, &p);
    let tool = DelegationTool("delegation_plan");
    let first = tool.call_enabled(&ctx, &args).unwrap();
    let mut disabled = p.clone();
    disabled.enabled = false;
    ctx.store()
        .unwrap()
        .save_expert(disabled, Some(p.revision))
        .unwrap();
    assert_eq!(tool.call_enabled(&ctx, &args).unwrap(), first);
    assert_eq!(ctx.store().unwrap().load_agents().unwrap().len(), 1);
    let mut different = args;
    different["tasks"][0]["brief"] = json!("Different input");
    assert!(tool.call_enabled(&ctx, &different).is_err());
}

#[test]
fn forged_authorization_and_outside_repository_are_rejected() {
    let (_dir, ctx, source, p) = fixture();
    let mut args = plan(&ctx, source, &p);
    args["authorization_id"] = json!(Uuid::new_v4());
    assert!(DelegationTool("delegation_plan")
        .call_enabled(&ctx, &args)
        .is_err());
    args["authorization_id"] = json!(source);
    args["tasks"][0]["repository"] = json!("/");
    assert!(DelegationTool("delegation_plan")
        .call_enabled(&ctx, &args)
        .is_err());
    assert_eq!(ctx.store().unwrap().load_agents().unwrap().len(), 1);
}

#[test]
fn relative_repository_is_resolved_against_the_lead_and_persisted_absolute() {
    let (_dir, ctx, source, p) = fixture();
    let mut args = plan(&ctx, source, &p);
    args["tasks"][0]["repository"] = json!(".");
    let result = content(
        DelegationTool("delegation_plan")
            .call_enabled(&ctx, &args)
            .unwrap(),
    );
    let run = ctx
        .store()
        .unwrap()
        .load_delegation(Uuid::parse_str(result["run_id"].as_str().unwrap()).unwrap())
        .unwrap();
    let parent = ctx.store().unwrap().load_agents().unwrap().remove(0);
    assert_eq!(
        run.tasks[0].plan.repository,
        parent.runtime_path().canonicalize().unwrap()
    );
}

#[test]
fn child_scope_cannot_read_sibling_or_operate_as_lead_and_stale_result_is_rejected() {
    let (_dir, ctx, source, p) = fixture();
    let answer = content(
        DelegationTool("delegation_plan")
            .call_enabled(&ctx, &plan(&ctx, source, &p))
            .unwrap(),
    );
    let run_id = Uuid::parse_str(answer["run_id"].as_str().unwrap()).unwrap();
    let store = ctx.store().unwrap();
    let child = Uuid::new_v4();
    let attempt = Uuid::new_v4();
    store
        .update_delegation(run_id, None, |r| {
            r.status = RunStatus::Active;
            let t = &mut r.tasks[0];
            t.status = TaskStatus::Running;
            t.attempts.push(DelegationAttempt {
                id: attempt,
                child_agent_id: child,
                generation: 1,
                workspace: "unused-fixture".into(),
                snapshot: None,
                completed_snapshot: None,
                archive_snapshot: None,
                working_copy_cleaned: false,
                result: None,
                result_revision: None,
                report_requested: false,
                session_id: None,
                progress: String::new(),
                usage: None,
            });
            Ok(())
        })
        .unwrap();
    let run = store.load_delegation(run_id).unwrap();
    let task = run.tasks[0].id;
    let childctx = ServerContext {
        project_id: ctx.project_id,
        agent_id: Some(child),
        store: ctx.store.clone(),
        delegation_scope: Some(DelegationBinding {
            run_id,
            parent_agent_id: run.parent_agent_id,
            task_id: Some(task),
            attempt_id: Some(attempt),
            workspace: None,
            task_kind: Some(run.tasks[0].plan.kind),
        }),
    };
    let read = content(
        DelegationTool("delegation_read")
            .call_enabled(&childctx, &json!({"run_id":run_id}))
            .unwrap(),
    );
    assert_eq!(read["tasks"].as_array().unwrap().len(), 1);
    assert!(DelegationTool("delegation_finish").call_enabled(&childctx,&json!({"run_id":run_id,"operation_key":"forged","expected_revision":run.revision,"summary":"done","verification":"claim"})).is_err());
    assert!(DelegationTool("delegation_complete").call_enabled(&childctx,&json!({"run_id":run_id,"operation_key":"old-result","expected_revision":run.revision,"task_id":task,"attempt_id":attempt,"task_revision":99,"result":{"summary":"Done","addressed":[],"checks":[],"unresolved":[]}})).is_err());
    let mut oldctx = childctx;
    oldctx.delegation_scope.as_mut().unwrap().attempt_id = Some(Uuid::new_v4());
    assert!(DelegationTool("delegation_read")
        .call_enabled(&oldctx, &json!({"run_id":run_id}))
        .is_err());
}

#[test]
fn context_omits_hidden_reasoning_and_preserves_pagination() {
    let (_dir, ctx, source, p) = fixture();
    let store = ctx.store().unwrap();
    let parent = ctx.agent_id().unwrap();
    for n in 0..14 {
        store
            .append_chat_message(parent, "user", format!("Source {n}"), 0, None)
            .unwrap();
    }
    store
        .append_chat_message(parent, "thought", "PRIVATE REASONING", 0, None)
        .unwrap();
    let answer = content(
        DelegationTool("delegation_plan")
            .call_enabled(&ctx, &plan(&ctx, source, &p))
            .unwrap(),
    );
    let read = content(
        DelegationTool("delegation_read")
            .call_enabled(
                &ctx,
                &json!({"run_id":answer["run_id"],"include_context":true}),
            )
            .unwrap(),
    );
    assert!(!read.to_string().contains("PRIVATE REASONING"));
    assert!(read["context"]["older_context_before"].is_number());
    assert_eq!(read["context"]["excerpts"].as_array().unwrap().len(), 12);
}

#[test]
fn temporary_teammates_share_inherited_config_but_have_distinct_tasks() {
    let (_dir, ctx, _, _) = fixture();
    let store = ctx.store().unwrap();
    let parent = store.load_agents().unwrap()[0].clone();
    let count = store.load_experts().unwrap().len();
    let source = Uuid::new_v4();
    store
        .authorize_experts(
            parent.id,
            source,
            "Please delegate research and testing",
            &[],
            false,
        )
        .unwrap()
        .unwrap();
    let listed = content(
        DelegationTool("experts_list")
            .call(&ctx, &json!({}))
            .unwrap(),
    );
    let temporary = listed["on_demand_expert_ids"][0].as_str().unwrap();
    let tasks = ["Research editors", "Test integration"].iter().enumerate().map(|(i, goal)| json!({
        "key":format!("task-{i}"), "expert_id":temporary, "goal":goal,
        "brief":"Inspect relevant files and report evidence", "expected_outcome":"Findings with sources",
        "repository":parent.project_path, "kind":"consultation"
    })).collect::<Vec<_>>();
    let args = json!({"authorization_id":source,"operation_key":"temporary-plan","expected_revision":0,"tasks":tasks});
    DelegationTool("delegation_plan").call(&ctx, &args).unwrap();
    DelegationTool("delegation_plan").call(&ctx, &args).unwrap();
    let runs = store.load_delegations().unwrap();
    assert_eq!(runs.len(), 1);
    let run = &runs[0];
    assert_eq!(run.tasks.len(), 2);
    assert_ne!(run.tasks[0].id, run.tasks[1].id);
    assert_ne!(
        run.tasks[0].expert.profile.name,
        run.tasks[1].expert.profile.name
    );
    for task in &run.tasks {
        assert_eq!(task.expert.profile.provider, parent.provider);
        assert_eq!(task.expert.profile.model, parent.model);
        assert_eq!(task.expert.profile.effort, parent.effort);
    }
    assert_eq!(store.load_experts().unwrap().len(), count);
    store
        .update_delegation(run.id, None, |run| {
            run.status = RunStatus::Paused;
            Ok(())
        })
        .unwrap();
    let mut later = args.clone();
    later["run_id"] = json!(run.id);
    later["expected_revision"] = json!(store.load_delegation(run.id).unwrap().revision);
    later["operation_key"] = json!("paused-plan");
    assert!(DelegationTool("delegation_plan")
        .call(&ctx, &later)
        .is_err());
}

#[test]
fn ordinary_text_cannot_authorize_temporary_teammates() {
    let (_dir, ctx, _, _) = fixture();
    let store = ctx.store().unwrap();
    let parent = ctx.agent_id().unwrap();
    let source = Uuid::new_v4();
    assert!(store
        .authorize_experts(parent, source, "Build a page", &[], false)
        .unwrap()
        .is_none());
    assert!(store.begin_delegation(parent, source).is_err());
    let listed = content(
        DelegationTool("experts_list")
            .call(&ctx, &json!({}))
            .unwrap(),
    );
    assert_eq!(listed["on_demand_expert_ids"], json!([]));
}
