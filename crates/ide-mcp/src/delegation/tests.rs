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
        .save_beta_features(ide_core::config::BetaFeatures { delegation: true, ..Default::default() })
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
        review_run: None,
        studio: false,
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
    let request = "Please add a new landing page and delegate expets ui designder fro deisgn, ux writie for texrt and delegate implementation to frotnend";
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
fn later_work_scope_cannot_authorize_another_bandmate() {
    let (_dir, ctx, _, _) = fixture();
    let store = ctx.store().unwrap();
    store.ensure_default_experts().unwrap();
    let source = Uuid::new_v4();
    let grant = store
        .authorize_experts(
            ctx.agent_id().unwrap(),
            source,
            "Delegate the layout to UI Designer. Also update the backend and frontend styling.",
            &[],
            false,
        )
        .unwrap()
        .unwrap();
    let profiles = store.load_experts().unwrap();
    let ui = profiles.iter().find(|p| p.name == "UI Designer").unwrap();
    let frontend = profiles
        .iter()
        .find(|p| p.name == "Frontend Engineer")
        .unwrap();
    assert_eq!(grant.expert_ids, vec![ui.id]);
    let error = DelegationTool("delegation_plan")
        .call_enabled(&ctx, &plan(&ctx, source, frontend))
        .unwrap_err();
    assert!(
        error.to_string().contains("not named this Bandmate"),
        "{error:#}"
    );
    // Run creation precedes plan validation, but a rejected plan must neither
    // extend its authorized team nor create assignments or child chats.
    let runs = store.load_delegations().unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].authorized_experts, vec![ui.id]);
    assert!(runs[0].tasks.is_empty());
    assert_eq!(store.load_agents().unwrap().len(), 1);
    assert_eq!(
        store
            .latest_expert_authorization(ctx.agent_id().unwrap())
            .unwrap()
            .unwrap()
            .expert_ids,
        vec![ui.id]
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
    assert!(error.contains("No delegation authority was recorded"));
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
            .contains("Settings → Band → Delegation"));
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
        review_run: None,
        studio: false,
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
fn natural_on_demand_request_stays_ready_after_clarification_without_a_picker() {
    let (_dir, ctx, _, _) = fixture();
    let store = ctx.store().unwrap();
    let parent = store.load_agents().unwrap()[0].clone();
    let source = Uuid::new_v4();
    let id = store.prepare_delegation_submission(parent.id, source,
        "I want to \"redesign\" our video using its current source and compare different approaches. Please delagte four on-demand teammates without requiring saved profiles.",
        &[], false).unwrap().unwrap();
    store
        .prepare_delegation_submission(
            parent.id,
            Uuid::new_v4(),
            "They can simply be created on demand without specific profiles",
            &[],
            false,
        )
        .unwrap();
    let listed = content(
        DelegationTool("experts_list")
            .call(&ctx, &json!({}))
            .unwrap(),
    );
    assert_eq!(listed["authorization_status"], "ready");
    assert_eq!(listed["on_demand_expert_ids"], json!([source]));
    assert_eq!(listed["active_run"]["id"], json!(id));
    let tasks = (0..4).map(|i| json!({
        "key":format!("approach-{i}"), "expert_id":source,
        "goal":format!("Propose video approach {i}"), "brief":"Inspect the source and propose an approach",
        "expected_outcome":"A concrete proposal with evidence", "repository":parent.project_path,
        "kind":"consultation"
    })).collect::<Vec<_>>();
    let args = json!({"authorization_id":listed["authorization_id"],"operation_key":"natural-plan",
        "expected_revision":0,"tasks":tasks});
    DelegationTool("delegation_plan").call(&ctx, &args).unwrap();
    DelegationTool("delegation_plan").call(&ctx, &args).unwrap();
    assert_eq!(store.load_delegation(id).unwrap().tasks.len(), 4);
    assert_eq!(store.load_delegations().unwrap().len(), 1);
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

#[test]
fn on_demand_model_comparison_resolves_user_typos_without_changing_the_lead() {
    let (_dir, ctx, _, _) = fixture();
    let store = ctx.store().unwrap();
    let parent = store.load_agents().unwrap()[0].clone();
    let profiles = store.load_experts().unwrap();
    let source = Uuid::new_v4();
    store.authorize_experts(parent.id, source,
        "Delegate four on-demand teammates using GPT6 asrta, GPT-6 Sol, Fabel 5.1 and Sonnet 5. Keep outputs separate and compare results.", &[], false).unwrap().unwrap();
    let listed = content(
        DelegationTool("experts_list")
            .call(&ctx, &json!({}))
            .unwrap(),
    );
    let temporary = listed["on_demand_expert_ids"][0].as_str().unwrap();
    let names = ["GPT6 asrta", "GPT-6 Sol", "Fabel 5.1", "Sonnet 5"];
    let tasks = names.iter().enumerate().map(|(i, name)| json!({
        "key":format!("variant-{i}"), "expert_id":temporary, "model_request":name,
        "goal":format!("Create variant {i}"), "brief":"Use the same source brief and keep output separate",
        "expected_outcome":"Independent deliverable with checks", "repository":parent.project_path, "kind":"implementation"
    })).collect::<Vec<_>>();
    let args = json!({"authorization_id":source,"operation_key":"model-comparison","expected_revision":0,"tasks":tasks});
    let first = DelegationTool("delegation_plan").call(&ctx, &args).unwrap();
    assert_eq!(
        DelegationTool("delegation_plan").call(&ctx, &args).unwrap(),
        first
    );
    let runs = store.load_delegations().unwrap();
    let run = &runs[0];
    assert_eq!(run.tasks.len(), 4);
    assert_eq!(
        run.tasks
            .iter()
            .map(|t| t.expert.profile.model)
            .collect::<Vec<_>>(),
        vec![
            AgentModel::CodexGpt6Astra,
            AgentModel::CodexGpt6Sol,
            AgentModel::ClaudeFable51,
            AgentModel::ClaudeSonnet
        ]
    );
    let mut schedulable = run.clone();
    schedulable.status = RunStatus::Active;
    assert_eq!(
        schedulable.ready_tasks(0).len(),
        3,
        "Concurrency limits still apply"
    );
    assert_eq!(store.load_agents().unwrap()[0].model, parent.model);
    assert_eq!(store.load_agents().unwrap()[0].provider, parent.provider);
    assert_eq!(store.load_experts().unwrap(), profiles);
    assert_eq!(
        run.temporary_model_authorizations.get(&source).unwrap(),
        &run.original_assignment
    );
}

#[test]
fn model_requests_fail_closed_for_unknown_ambiguous_and_unmentioned_names() {
    for (typed, requested) in [
        ("Fable", "Fable"),
        ("Gemini", "Gemini"),
        ("Astra", "Sonnet 5"),
        ("Sonnnet 5", "Sonnet 5"),
    ] {
        let (_dir, ctx, _, _) = fixture();
        let store = ctx.store().unwrap();
        let parent = store.load_agents().unwrap()[0].clone();
        let source = Uuid::new_v4();
        store
            .authorize_experts(
                parent.id,
                source,
                &format!("Delegate a teammate using {typed}"),
                &[],
                false,
            )
            .unwrap()
            .unwrap();
        let args = json!({"authorization_id":source,"operation_key":"invalid-model","expected_revision":0,
            "tasks":[{"key":"variant","expert_id":source,"model_request":requested,"goal":"Build variant",
            "brief":"Build the requested output","expected_outcome":"Checked","repository":parent.project_path,"kind":"implementation"}]});
        assert!(
            DelegationTool("delegation_plan").call(&ctx, &args).is_err(),
            "{typed}/{requested}"
        );
        assert!(store.load_delegations().unwrap()[0].tasks.is_empty());
        assert_eq!(store.load_agents().unwrap().len(), 1);
    }
}

#[test]
fn followup_model_authority_is_bound_to_its_own_temporary_teammate() {
    let (_dir, ctx, _, _) = fixture();
    let store = ctx.store().unwrap();
    let parent = store.load_agents().unwrap()[0].clone();
    let first = Uuid::new_v4();
    store
        .authorize_experts(parent.id, first, "Delegate using Astra", &[], false)
        .unwrap();
    let run = store.begin_delegation(parent.id, first).unwrap();
    let second = Uuid::new_v4();
    store
        .authorize_experts(
            parent.id,
            second,
            "Delegate another teammate using Sonnet 5",
            &[],
            false,
        )
        .unwrap();
    let updated = store.begin_delegation(parent.id, second).unwrap();
    assert_eq!(run.id, updated.id);
    let mut args = json!({"run_id":run.id,"operation_key":"followup-model","expected_revision":updated.revision,
        "tasks":[{"key":"followup","expert_id":first,"model_request":"Sonnet 5","goal":"Another variant",
        "brief":"Use the user requested model","expected_outcome":"Checked","repository":parent.project_path,"kind":"consultation"}]});
    assert!(DelegationTool("delegation_plan").call(&ctx, &args).is_err());
    args["tasks"][0]["expert_id"] = json!(second);
    DelegationTool("delegation_plan").call(&ctx, &args).unwrap();
    assert_eq!(
        store.load_delegation(run.id).unwrap().tasks[0]
            .expert
            .profile
            .model,
        AgentModel::ClaudeSonnet
    );
}

#[test]
fn ended_band_can_be_requested_again_with_a_correction_and_model_context() {
    let (_dir, ctx, _, _) = fixture();
    let store = ctx.store().unwrap();
    let parent = ctx.agent_id().unwrap();
    let first = store
        .prepare_delegation_submission(
            parent,
            Uuid::new_v4(),
            "Please delegate four teammates using GPT-6 Astra and GPT-6 Sol",
            &[],
            false,
        )
        .unwrap()
        .unwrap();
    store
        .update_delegation(first, None, |r| {
            r.status = RunStatus::Cancelled;
            Ok(())
        })
        .unwrap();
    assert!(store
        .prepare_delegation_submission(
            parent,
            Uuid::new_v4(),
            "Sorry, I mean three teammates: GPT-6 Astra, GPT-6 Sol and Sonnet 5",
            &[],
            false
        )
        .unwrap()
        .is_none());
    let source = Uuid::new_v4();
    let second = store
        .prepare_delegation_submission(parent, source, "please deleage it", &[], false)
        .unwrap()
        .unwrap();
    assert_ne!(first, second);
    let listed = content(
        DelegationTool("experts_list")
            .call(&ctx, &json!({}))
            .unwrap(),
    );
    assert_eq!(listed["authorization_status"], "ready");
    let repo = store.load_agents().unwrap()[0].project_path.clone();
    let tasks = ["GPT-6 Astra", "GPT-6 Sol", "Sonnet 5"].iter().enumerate().map(|(i, model)| json!({
        "key":format!("variant-{i}"), "expert_id":source, "model_request":model,
        "goal":"An independent video approach", "brief":"Keep voiceover and compare the result",
        "expected_outcome":"A checked proposal", "repository":repo, "kind":"consultation"
    })).collect::<Vec<_>>();
    let args = json!({"run_id":second,"operation_key":"retry-three","expected_revision":listed["active_run"]["revision"],"tasks":tasks});
    DelegationTool("delegation_plan").call(&ctx, &args).unwrap();
    DelegationTool("delegation_plan").call(&ctx, &args).unwrap();
    assert_eq!(store.load_delegation(second).unwrap().tasks.len(), 3);
    assert_eq!(
        store.load_delegation(first).unwrap().status,
        RunStatus::Cancelled
    );
}

#[test]
fn adding_a_teammate_during_a_run_preserves_existing_assignments() {
    let (_dir, ctx, source, expert) = fixture();
    let store = ctx.store().unwrap();
    DelegationTool("delegation_plan")
        .call(&ctx, &plan(&ctx, source, &expert))
        .unwrap();
    let before = store.load_delegations().unwrap().pop().unwrap();
    let added = Uuid::new_v4();
    assert_eq!(
        store
            .prepare_delegation_submission(
                ctx.agent_id().unwrap(),
                added,
                "Add another teammate to check the tests",
                &[],
                false
            )
            .unwrap(),
        Some(before.id)
    );
    let listed = content(
        DelegationTool("experts_list")
            .call(&ctx, &json!({}))
            .unwrap(),
    );
    let args = json!({"run_id":before.id,"operation_key":"add-tester","expected_revision":listed["active_run"]["revision"],"tasks":[{
        "key":"tester", "expert_id":added,"goal":"Check the tests", "brief":"Review tests in the repository",
        "expected_outcome":"Evidence and gaps", "repository":before.tasks[0].plan.repository,"kind":"consultation"
    }]});
    DelegationTool("delegation_plan").call(&ctx, &args).unwrap();
    let after = store.load_delegation(before.id).unwrap();
    assert_eq!(after.tasks.len(), before.tasks.len() + 1);
    assert_eq!(after.tasks[0].id, before.tasks[0].id);
    assert_eq!(after.tasks[0].revision, before.tasks[0].revision);
}

#[test]
fn retry_model_authority_is_not_a_global_preference() {
    for (status, request) in [
        (
            RunStatus::Cancelled,
            "Delegate a teammate to build another feature",
        ),
        (RunStatus::Completed, "Please delegate it again"),
    ] {
        let (_dir, ctx, _, _) = fixture();
        let store = ctx.store().unwrap();
        let parent = ctx.agent_id().unwrap();
        let first = store
            .prepare_delegation_submission(
                parent,
                Uuid::new_v4(),
                "Delegate an on-demand teammate using Sonnet 5",
                &[],
                false,
            )
            .unwrap()
            .unwrap();
        store
            .update_delegation(first, None, |r| {
                r.status = status;
                Ok(())
            })
            .unwrap();
        let source = Uuid::new_v4();
        let second = store
            .prepare_delegation_submission(parent, source, request, &[], false)
            .unwrap()
            .unwrap();
        let run = store.load_delegation(second).unwrap();
        assert_eq!(run.temporary_model_authorizations[&source], request);
        let repo = store.load_agents().unwrap()[0].project_path.clone();
        let args = json!({"run_id":second,"operation_key":"unauthorized-model","expected_revision":run.revision,"tasks":[{
            "key":"different-task", "expert_id":source,"model_request":"Sonnet 5", "goal":"New work",
            "brief":"New task", "expected_outcome":"Checked result", "repository":repo,"kind":"consultation"
        }]});
        assert!(DelegationTool("delegation_plan").call(&ctx, &args).is_err());
        assert!(store.load_delegation(second).unwrap().tasks.is_empty());
    }
}
