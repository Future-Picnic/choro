//! Choro delegation tools queue durable work; the application owns runtimes.
#[cfg(test)]
#[path = "delegation/tests.rs"]
mod tests;
use super::{text_content, ServerContext, Tool};
use anyhow::{anyhow, ensure, Context, Result};
use ide_core::delegation::{RunStatus, TaskKind, TaskPlan, TaskResult, TaskStatus, WaitCondition};
use serde_json::{json, Value};
use uuid::Uuid;

pub(super) fn tools() -> Vec<Box<dyn Tool>> {
    [
        "experts_list",
        "delegation_plan",
        "delegation_read",
        "delegation_message",
        "delegation_control",
        "delegation_complete",
        "delegation_integrate",
        "delegation_wait",
        "delegation_finish",
    ]
    .into_iter()
    .map(|name| Box::new(DelegationTool(name)) as Box<dyn Tool>)
    .collect()
}

struct DelegationTool(&'static str);
fn uuid(args: &Value, key: &str) -> Result<Uuid> {
    Uuid::parse_str(
        args.get(key)
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("Provide {key}."))?,
    )
    .with_context(|| format!("Invalid {key}."))
}
fn string<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("Provide {key}."))
}

impl Tool for DelegationTool {
    fn name(&self) -> &'static str {
        self.0
    }
    fn title(&self) -> &'static str {
        match self.0 {
            "experts_list" => "Find Bandmates",
            "delegation_plan" => "Assign Bandmate tasks",
            "delegation_read" => "Read delegated work",
            "delegation_message" => "Coordinate with lead or Bandmate",
            "delegation_control" => "Update an assignment",
            "delegation_complete" => "Report Bandmate result",
            "delegation_integrate" => "Integrate Bandmate changes",
            "delegation_wait" => "Wait for Bandmates",
            _ => "Finish delegated task",
        }
    }
    fn description(&self) -> &'static str {
        match self.0 {
            "experts_list" => "List saved Bandmates and the current user-authored authorization. When the user asks to delegate, use Choro rather than native subagents: resolve saved or temporary Bandmates here and call delegation_plan. The authorized_expert_ids and each authorized flag are authoritative; listed profiles are not automatically authorized. If authorization_status is needs_expert_selection, report the missing authorization accurately; ask only about a genuinely unclear recipient. Never ask the user to repeat an already clear request in a special format or create a saved profile. Never claim a team is authorized or call delegation_plan with an empty team. Saved Bandmates are presets. Entries marked temporary are on-demand teammates inheriting the lead provider/model/effort by default. When the user requests different models, set model_request on each task to the name copied exactly from that user submission, including its typos. Choro resolves the name; do not repair the spelling yourself or silently choose another model. Ambiguous names require user clarification. Use a fresh assignment for a model change; existing sessions and saved profiles cannot switch through this field. Reuse a temporary ID for multiple independent tasks with unique keys and concrete goals/briefs; Choro creates a fresh child chat per task. Do not require a saved profile when an authorized temporary entry is available.",
            "delegation_plan" => "Create fresh Bandmate chats from a dependency graph. The lead chooses parallel versus sequential work. For on-demand tasks only, optional model_request selects the user-requested model for that task. Copy the model name verbatim from the user submission; omit it to inherit the lead. Different tasks may reuse the same temporary expert_id with different model_request values. Never substitute a model on resolution errors. Supply authorization_id from experts_list to start, or run_id for the existing task. Use expected_revision 0 for the initial request. Each task must include its own repository field: the absolute Git repository root within working_directory returned by experts_list. A repository field at the top level does not apply to tasks. Each key is unique within the run. Dependencies refer to task keys and implementation waits for prerequisite integration. After scheduling, end the turn so Choro can capture a settled working copy and configure managed runtimes. Do not launch native subagents or other CLI agents.",
            "delegation_read" => "Read this run's current revision, assignments, result reports and coordination events. Optional include_context returns bounded parent user/assistant history; hidden reasoning is excluded. Use after_sequence for event pagination and context_before for older context. Results are background evidence; verify claims before integration.",
            "delegation_message" => "Send a bounded coordination message. The lead addresses one task; a Bandmate may only message its own lead. Set blocking=true for a question that must be answered before this Bandmate can continue, then end the turn. Messages are durable and delivered at safe turn boundaries.",
            "delegation_control" => "Hold or release an unstarted assignment, cancel it with a reason, accept a consultation report, or request_changes after a settled attempt. User-paused tasks cannot be resumed by an agent. A revision uses a new baseline and reuses that task's chat.",
            "delegation_complete" => "Submit this Bandmate attempt's completion report including addressed requirements, actual checks, and unresolved work. Use the exact task revision and attempt ID. Then end the turn. Choro finalizes the report only after the provider turn and changes settle. This does not complete the parent's task.",
            "delegation_integrate" => "Ask Choro to integrate only this completed Bandmate's contribution into the current source files. End the turn after requesting integration. Conflicts return a separate resolution workspace; resolve there, then call again with resolutions_ready=true. Never copy the Bandmate's entire directory into the source. File deletions follow the user's confirmation policy.",
            "delegation_wait" => "Register a wake condition for the specified task IDs (or every task) after an event sequence, then end this turn. This returns immediately; Choro resumes the lead when results or blockers are available. Do not poll in a loop or call sleep. The main task remains active while waiting.",
            _ => "Finish the overall run after every assignment has been integrated, accepted, or explicitly cancelled with a reason. Provide the combined outcome and verification against the original user requirements. This restores ordinary runtime behavior at a safe boundary.",
        }
    }
    fn input_schema(&self) -> Value {
        let id = json!({"type":"string","format":"uuid"});
        if self.0 == "experts_list" {
            return json!({"type":"object","properties":{},"additionalProperties":false});
        }
        if self.0 == "delegation_read" {
            return json!({"type":"object","properties":{"run_id":id,"after_sequence":{"type":"integer","minimum":0},"include_context":{"type":"boolean"},"context_before":{"type":"integer"}},"required":["run_id"],"additionalProperties":false});
        }
        let mut properties = json!({"run_id":id,"operation_key":{"type":"string","minLength":1,"maxLength":160},"expected_revision":{"type":"integer","minimum":0}});
        let mut required = vec!["run_id", "operation_key", "expected_revision"];
        let extras = match self.0 {
            "delegation_plan" => {
                required.remove(0);
                required.push("tasks");
                json!({"authorization_id":id,"tasks":{"type":"array","minItems":1,"maxItems":64,"items":{"type":"object","properties":{"key":{"type":"string"},"expert_id":id,"model_request":{"type":"string","minLength":1,"maxLength":120,"description":"Optional for on-demand teammates only: copy the model name exactly as the user typed it, including typos. Choro resolves unique supported names and rejects ambiguous or unauthorized choices. Omit to inherit the lead model."},"goal":{"type":"string"},"brief":{"type":"string"},"expected_outcome":{"type":"string"},"repository":{"type":"string"},"dependencies":{"type":"array","items":{"type":"string"}},"kind":{"enum":["implementation","consultation"]},"held":{"type":"boolean"}},"required":["key","expert_id","goal","brief","expected_outcome","repository","kind"],"additionalProperties":false}}})
            }
            "delegation_message" => {
                required.extend(["task_id", "message"]);
                json!({"task_id":id,"message":{"type":"string","maxLength":16000},"blocking":{"type":"boolean"}})
            }
            "delegation_control" => {
                required.extend(["task_id", "action", "reason"]);
                json!({"task_id":id,"action":{"enum":["hold","release","cancel","accept","request_changes"]},"reason":{"type":"string","maxLength":16000}})
            }
            "delegation_complete" => {
                required.extend(["task_id", "attempt_id", "task_revision", "result"]);
                json!({"task_id":id,"attempt_id":id,"task_revision":{"type":"integer","minimum":1},"result":{"type":"object","properties":{"summary":{"type":"string","maxLength":16000},"addressed":{"type":"array","items":{"type":"string"}},"checks":{"type":"array","items":{"type":"string"}},"unresolved":{"type":"array","items":{"type":"string"}}},"required":["summary","addressed","checks","unresolved"],"additionalProperties":false}})
            }
            "delegation_integrate" => {
                required.extend(["task_id", "attempt_id", "result_revision"]);
                json!({"task_id":id,"attempt_id":id,"result_revision":{"type":"integer","minimum":1},"resolutions_ready":{"type":"boolean"}})
            }
            "delegation_wait" => {
                required.push("after_sequence");
                json!({"after_sequence":{"type":"integer","minimum":0},"task_ids":{"type":"array","items":id}})
            }
            _ => {
                required.extend(["summary", "verification"]);
                json!({"summary":{"type":"string","maxLength":16000},"verification":{"type":"string","maxLength":32000}})
            }
        };
        properties
            .as_object_mut()
            .unwrap()
            .extend(extras.as_object().unwrap().clone());
        json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
    }
    fn call(&self, ctx: &ServerContext, args: &Value) -> Result<Vec<Value>> {
        ensure!(
            ide_core::delegation::enabled(),
            "Bandmates are not enabled in this Choro build."
        );
        self.call_enabled(ctx, args)
    }
}

impl DelegationTool {
    fn call_enabled(&self, ctx: &ServerContext, args: &Value) -> Result<Vec<Value>> {
        let store = ctx.store()?;
        let caller = ctx.agent_id()?;
        if self.0 == "experts_list" {
            ensure!(
                store.delegation_available_for(caller)?,
                "{}",
                ide_core::delegation::BETA_DISABLED
            );
            let authorization = store.latest_expert_authorization(caller)?;
            let active = store
                .load_delegations()?
                .into_iter()
                .find(|r| r.parent_agent_id == caller && !r.status.terminal());
            let authorized = active
                .as_ref()
                .map(|r| r.authorized_experts.clone())
                .or_else(|| authorization.as_ref().map(|a| a.expert_ids.clone()))
                .unwrap_or_default();
            let temporary = active
                .as_ref()
                .map(|r| r.temporary_experts.clone())
                .or_else(|| authorization.as_ref().map(|a| a.temporary_experts.clone()))
                .unwrap_or_default();
            let working_directory = store
                .load_agents()?
                .into_iter()
                .find(|a| a.id == caller)
                .map(|a| a.runtime_path().to_path_buf());
            let authorization_status = if authorized.is_empty() {
                "needs_expert_selection"
            } else {
                "ready"
            };
            let guidance = if authorized.is_empty() {
                "No delegation authority was recorded for the current user submission. Do not call delegation_plan or describe the team as authorized. If the user already clearly requested delegation, report this as an authorization problem; do not ask them to retype it or use /delegate. Ask a focused recipient question only if their intent is genuinely unclear."
            } else {
                "Proceed using IDs in authorized_expert_ids without asking the user to repeat their request, use /delegate, or create profiles. When active_run is present, pass its id as run_id and its revision as expected_revision to delegation_plan; authorization_id is only needed to begin a new run. Include repository on every task. An authorized on-demand ID may serve multiple roles within the user’s task: provide a separate key, goal, brief and expected outcome for each. When the user asks for a model, pass its original spelling as model_request on the temporary task; distinct tasks may use different models. Names must come from the user-authored context that authorized that temporary ID; an explicit retry may reference the same task's earlier user messages. Saved profiles still require their own authorized IDs."
            };
            let mut profiles = store.load_experts()?.into_iter().filter(|p| !p.archived).map(|p| {
                let error = p.snapshot_at(store.root()).err().map(|e| e.to_string());
                json!({"id":p.id,"name":p.name,"description":p.description,"aliases":ide_core::experts::expert_aliases(&p),"provider":p.provider,"model":p.model,"enabled":p.enabled,"authorized":authorized.contains(&p.id),"configuration_error":error})
            }).collect::<Vec<_>>();
            profiles.extend(temporary.iter().map(|e| {
                json!({
                    "id":e.profile.id,"name":e.profile.name,"description":e.profile.description,
                    "provider":e.profile.provider,"model":e.profile.model,"effort":e.profile.effort,
                    "enabled":true,"authorized":authorized.contains(&e.profile.id),"temporary":true,
                    "configuration_error":null
                })
            }));
            let model_choices = [ide_core::AgentKind::Codex, ide_core::AgentKind::Claude]
                .into_iter().flat_map(|provider| {
                    ide_core::AgentModel::models_for(provider).iter().map(move |model| {
                        json!({"provider":provider,"label":model.label(),"short_label":model.short_label(),"model":model,"cli_name":model.cli_value()})
                    })
                }).collect::<Vec<_>>();
            return Ok(vec![text_content(serde_json::to_string(
                &json!({"experts":profiles,"model_choices":model_choices,"on_demand_expert_ids":temporary.iter().map(|e|e.profile.id).collect::<Vec<_>>(),"authorized_expert_ids":authorized,"authorization_status":authorization_status,"guidance":guidance,"working_directory":working_directory,"authorization_id":authorization.map(|a| a.id),"active_run":active.map(|r|json!({"id":r.id,"revision":r.revision,"status":r.status}))}),
            )?)]);
        }
        if self.0 != "delegation_read" {
            let key = string(args, "operation_key")?;
            ensure!(
                !key.trim().is_empty() && key.len() <= 160,
                "Provide a bounded operation key."
            );
            args.get("expected_revision")
                .and_then(Value::as_u64)
                .context("Provide expected_revision.")?;
        }
        if self.0 == "delegation_plan" {
            let _: Vec<TaskPlan> =
                serde_json::from_value(args.get("tasks").cloned().context("Provide tasks.")?)
                    .map_err(|e| anyhow::anyhow!("Invalid assignment: {e}. Each task needs key, expert_id, goal, brief, expected_outcome, repository (its Git root), and kind. Use working_directory from experts_list to identify the repository; put repository inside each task, not at the top level."))?;
        }
        let run = if self.0 == "delegation_plan" && args.get("run_id").is_none() {
            store.begin_delegation(caller, uuid(args, "authorization_id")?)?
        } else {
            store.load_delegation(uuid(args, "run_id")?)?
        };
        run.authorize_caller(caller)?;
        let own_task = if caller == run.parent_agent_id {
            None
        } else {
            let scope=ctx.delegation_scope.as_ref().context("This MCP connection has no managed task scope. Reconnect this Bandmate through Choro.")?;
            ensure!(
                scope.run_id == run.id,
                "This MCP connection belongs to a different run."
            );
            let task = run.task(scope.task_id.context("Missing task scope")?)?;
            ensure!(
                task.attempt()
                    .is_some_and(|a| Some(a.id) == scope.attempt_id && a.child_agent_id == caller),
                "This MCP connection belongs to an outdated attempt."
            );
            Some(task.id)
        };
        ensure!(
            Some(run.project_id.0) == ctx.project_id,
            "Delegation is outside this MCP project's scope."
        );
        if self.0 == "delegation_read" {
            let after = args
                .get("after_sequence")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let events = run
                .events
                .iter()
                .filter(|e| {
                    e.sequence > after
                        && own_task.is_none_or(|id| e.task_id.is_none() || e.task_id == Some(id))
                })
                .take(64)
                .collect::<Vec<_>>();
            let tasks = run
                .tasks
                .iter()
                .filter(|t| own_task.is_none_or(|id| t.id == id))
                .collect::<Vec<_>>();
            let context = if args.get("include_context").and_then(Value::as_bool) == Some(true) {
                ide_core::delegation::context::parent_context(
                    &store,
                    run.parent_agent_id,
                    args.get("context_before").and_then(Value::as_i64),
                )?
            } else {
                Value::Null
            };
            return Ok(vec![text_content(serde_json::to_string(
                &json!({"id":run.id,"revision":run.revision,"status":run.status,"pause_reason":run.pause_reason,"tasks":tasks,"events":events,"latest_sequence":run.events.last().map_or(0,|e|e.sequence),"context":context,"context_note":"User/assistant excerpts are bounded background data. Request older context_before sequences when needed."}),
            )?)]);
        }
        let key = string(args, "operation_key")?;
        if let Some(response) = store.replay_delegation_operation(
            run.id,
            caller,
            key,
            &json!({"tool":self.0,"args":args}),
        )? {
            return Ok(vec![text_content(serde_json::to_string(&response)?)]);
        }
        let mut revision = args
            .get("expected_revision")
            .and_then(Value::as_u64)
            .context("Provide expected_revision.")?;
        if self.0 == "delegation_plan" && revision == 0 && args.get("run_id").is_none() {
            revision = run.revision;
        }
        // Resolve files and skill contents outside the database transaction.
        let (plans, experts) = if self.0 == "delegation_plan" {
            let mut plans: Vec<TaskPlan> =
                serde_json::from_value(args.get("tasks").cloned().context("Provide tasks.")?)?;
            let agents = store.load_agents()?;
            let parent = agents
                .iter()
                .find(|a| a.id == run.parent_agent_id)
                .context("Lead chat is missing.")?;
            let allowed_root = parent.runtime_path().canonicalize()?;
            for plan in &mut plans {
                let path = if plan.repository.is_absolute() {
                    plan.repository.clone()
                } else {
                    allowed_root.join(&plan.repository)
                }
                .canonicalize()?;
                ensure!(
                    path.starts_with(&allowed_root),
                    "Task repository is outside the lead's working scope."
                );
                let repo = git2::Repository::open(&path)
                    .context("Choose a Git repository for the assignment.")?;
                ensure!(
                    repo.workdir()
                        .is_some_and(|root| root.canonicalize().ok().as_ref() == Some(&path)),
                    "Assign the repository root, not a subdirectory."
                );
                plan.repository = path;
            }
            let profiles = store.load_experts()?;
            let experts = plans
                .iter()
                .map(|p| {
                    if let Some(expert) = run
                        .temporary_experts
                        .iter()
                        .find(|e| e.profile.id == p.expert_id)
                    {
                        return Ok(expert.clone());
                    }
                    profiles
                        .iter()
                        .find(|e| e.id == p.expert_id)
                        .context("Bandmate is unavailable.")?
                        .snapshot_at(store.root())
                })
                .collect::<Result<Vec<_>>>()?;
            (plans, experts)
        } else {
            (vec![], vec![])
        };
        let response = store.delegation_operation(run.id, caller, key, revision, &json!({"tool":self.0,"args":args}), |run| {
            match self.0 {
                "delegation_plan" => run.add_plans(caller, plans, &experts)?,
                "delegation_message" => run.message(caller, uuid(args, "task_id")?, string(args, "message")?.into(), args.get("blocking").and_then(Value::as_bool).unwrap_or(false))?,
                "delegation_control" => run.control(caller, uuid(args,"task_id")?, string(args,"action")?, string(args,"reason")?)?,
                "delegation_complete" => {
                    let result: TaskResult = serde_json::from_value(args.get("result").cloned().context("Provide result.")?)?;
                    run.complete(caller, uuid(args,"task_id")?, uuid(args,"attempt_id")?, args.get("task_revision").and_then(Value::as_u64).context("Provide task_revision.")?, result)?;
                }
                "delegation_integrate" => {
                    run.require_lead(caller)?; ensure!(!run.plan_mode, "Integration is paused in Plan mode.");
                    let task = run.task_mut(uuid(args,"task_id")?)?;
                    ensure!(task.plan.kind == TaskKind::Implementation && task.status == TaskStatus::ResultReady, "Only a settled implementation result can be integrated.");
                    let result_revision=args.get("result_revision").and_then(Value::as_u64).context("Provide result_revision.")?;
                    let attempt=task.attempt().context("Missing completed attempt.")?;
                    ensure!(attempt.id==uuid(args,"attempt_id")? && attempt.result_revision==Some(result_revision) && task.revision==result_revision,"This result is stale. Read the current task before integrating.");
                    task.status = TaskStatus::Integrating;
                    if args.get("resolutions_ready").and_then(Value::as_bool).unwrap_or(false) { task.reason = Some("resolutions_submitted".into()); }
                }
                "delegation_wait" => {
                    run.require_lead(caller)?;
                    let tasks: Vec<Uuid> = serde_json::from_value(args.get("task_ids").cloned().unwrap_or_else(|| json!([])))?;
                    for id in &tasks { run.task(*id)?; }
                    run.wait = Some(WaitCondition { after_sequence: args.get("after_sequence").and_then(Value::as_u64).context("Provide after_sequence.")?, tasks });
                    if run.status != RunStatus::Preparing { run.status = RunStatus::Waiting; }
                }
                "delegation_finish" => run.finish(caller, string(args,"summary")?.into(), string(args,"verification")?.into())?,
                _ => unreachable!(),
            }
            Ok(json!({"accepted":true,"run_id":run.id,"revision":run.revision + 1,"tasks":run.tasks.iter().map(|t|json!({"id":t.id,"key":t.plan.key,"status":t.status})).collect::<Vec<_>>(),"next":"End this turn when waiting, completing, or requesting file integration. Choro delivers results at a safe boundary."}))
        })?;
        Ok(vec![text_content(serde_json::to_string(&response)?)])
    }
}
