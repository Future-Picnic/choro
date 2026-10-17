//! Choro delegation tools queue durable work; the application owns runtimes.
#[cfg(test)]
#[path = "delegation/tests.rs"]
mod tests;
use super::{text_content, ServerContext, Tool};
use anyhow::{anyhow, ensure, Context, Result};
use ide_core::delegation::{RunStatus, TaskPlan, TaskResult, WaitCondition};
#[cfg(test)]
use ide_core::delegation::TaskStatus;
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

fn parse_plans(args: &Value) -> Result<Vec<TaskPlan>> {
    let mut tasks = args.get("tasks").cloned().context("Provide tasks.")?;
    for task in tasks.as_array_mut().context("tasks must be an array")? {
        let object = task.as_object_mut().context("Each task must be an object")?;
        if let Some(repository) = object.remove("repository") {
            if let Some(directory) = object.get("working_directory") {
                ensure!(*directory == repository, "repository and working_directory conflict. Supply only working_directory.");
            } else { object.insert("working_directory".into(), repository); }
        }
    }
    serde_json::from_value(tasks).context("Each task needs key, expert_id, goal, brief, expected_outcome, and kind; working_directory is optional.")
}

impl Tool for DelegationTool {
    fn name(&self) -> &'static str {
        self.0
    }
    fn title(&self) -> &'static str {
        match self.0 {
            "experts_list" => "List Bandmates",
            "delegation_plan" => "Assign Bandmate tasks",
            "delegation_read" => "Read delegated work",
            "delegation_message" => "Coordinate with lead or Bandmate",
            "delegation_control" => "Update an assignment",
            "delegation_complete" => "Report Bandmate result",

            "delegation_wait" => "Wait for Bandmates",
            _ => "Finish delegated task",
        }
    }
    fn description(&self) -> &'static str {
        match self.0 {
            "experts_list" => "List available saved Bandmates and on-demand teammates. When the user asks to delegate, select appropriate entries and call delegation_plan immediately. No separate authorization, special wording, or saved profile is required. Reuse an on-demand ID for several focused assignments. On-demand teammates inherit the lead provider, model and effort; use model_request only when the user requests a different model. Respect Stop, disabled profiles, and configuration errors.",
            "delegation_plan" => "Create fresh Bandmate chats with focused briefs and dependencies. Each agent works directly in the lead's existing folder; no project copies, snapshots, clones, worktrees, or merge step. Optional working_directory must stay within the lead's working scope; omit it to inherit that folder. Git is not required. Assign clear file responsibilities and sequence overlapping edits. Dependencies wait for lead acceptance. For temporary teammates only, copy any user-requested model name verbatim into model_request; omit to inherit. Omit run_id for new work; use the active run_id and revision for existing work. No authorization token is needed. End the turn after scheduling. Do not launch native subagents or other agent CLIs.",
            "delegation_read" => "Read this run's current revision, assignments, result reports and coordination events. Optional include_context returns bounded parent user/assistant history; hidden reasoning is excluded. Use after_sequence for event pagination and context_before for older context. Results are background evidence; verify claims and shared edits before acceptance.",
            "delegation_message" => "Send a bounded coordination message. The lead addresses one task; a Bandmate may only message its own lead. Set blocking=true for a question that must be answered before this Bandmate can continue, then end the turn. Messages are durable and delivered at safe turn boundaries.",
            "delegation_control" => "Hold or release an unstarted assignment, cancel it with a reason, accept a settled result report, or request_changes after a settled attempt. User-paused tasks cannot be resumed by an agent. A revision reuses the shared folder and that task's chat; it never copies or reverts files.",
            "delegation_complete" => "Submit this Bandmate attempt's completion report including addressed requirements, actual checks, and unresolved work. Use the exact task revision and attempt ID. Then end the turn. Choro finalizes the report only after the provider turn and changes settle. This does not complete the parent's task.",
            "delegation_wait" => "Register a wake condition for the specified task IDs (or every task) after an event sequence, then end this turn. This returns immediately; Choro resumes the lead when results or blockers are available. Do not poll in a loop or call sleep. The main task remains active while waiting.",
            _ => "Finish the overall run after every assignment has been accepted or explicitly cancelled with a reason. Provide the combined outcome and verification against the original user requirements. This restores ordinary runtime behavior at a safe boundary.",
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
                json!({"authorization_id":id,"tasks":{"type":"array","minItems":1,"maxItems":64,"items":{"type":"object","properties":{"key":{"type":"string"},"expert_id":id,"model_request":{"type":"string","minLength":1,"maxLength":120,"description":"Optional for on-demand teammates only: copy the model name exactly as the user typed it, including typos. Choro resolves unique supported names and rejects ambiguous or unsupported choices. Omit to inherit the lead model."},"goal":{"type":"string"},"brief":{"type":"string"},"expected_outcome":{"type":"string"},"working_directory":{"type":"string","description":"Existing directory within the lead’s working scope; defaults to the lead’s folder. Git is optional."},"repository":{"type":"string","description":"Deprecated alias for working_directory."},"dependencies":{"type":"array","items":{"type":"string"}},"kind":{"enum":["implementation","consultation"]},"held":{"type":"boolean"}},"required":["key","expert_id","goal","brief","expected_outcome","kind"],"additionalProperties":false}}})
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
        ensure!(self.0 != "delegation_integrate", "Bandmates now edit the shared project directly. Review the settled report and use delegation_control with action=accept or request_changes; integration is retired.");
        if self.0 != "delegation_read" {
            ensure!(!ctx.delegation_scope.as_ref().is_some_and(|b| b.is_legacy_child()), ide_core::delegation::LEGACY_CLOSED);
        }
        let store = ctx.store()?;
        let caller = ctx.agent_id()?;
        if matches!(self.0, "experts_list" | "delegation_plan") {
            let agents = store.load_agents()?;
            let parent = agents.iter().find(|a| a.id == caller).context("Lead chat is unavailable.")?;
            ensure!(Some(parent.project_id.0) == ctx.project_id, "Delegation is outside this MCP project's scope.");
        }
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
            let temporary = store.delegation_teammates(caller)?;
            let authorized = store.load_experts()?.into_iter()
                .filter(|p| p.enabled && !p.archived).map(|p| p.id)
                .chain(temporary.iter().map(|e| e.profile.id)).collect::<Vec<_>>();
            let working_directory = store.load_agents()?.into_iter()
                .find(|a| a.id == caller).map(|a| a.runtime_path().to_path_buf());
            // Compatibility fields describe availability, not user permission.
            let authorization_status = "ready";
            let guidance = "Choose an available saved Bandmate or on-demand teammate and call delegation_plan. No authorization token or extra user confirmation is required. For active work use active_run.id and its revision; for new work omit run_id and use expected_revision=0. Omit working_directory to share the lead's folder. Reuse an on-demand ID for multiple focused tasks. Follow the user's choice of model; omit model_request to inherit. A stopped run remains stopped until the user resumes it.";
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
        if self.0 == "delegation_plan" { parse_plans(args)?; }
        let run = if self.0 == "delegation_plan" && args.get("run_id").is_none() {
            store.begin_current_delegation(caller)?
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
        run.require_shared()?;
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
        let temporary = if self.0 == "delegation_plan" {
            store.delegation_teammates(caller)?
        } else {
            vec![]
        };
        // Resolve files and skill contents outside the database transaction.
        let (plans, experts) = if self.0 == "delegation_plan" {
            let mut plans = parse_plans(args)?;
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
                    "Task working directory is outside the lead's working scope."
                );
                ensure!(path.is_dir(), "Choose an existing working directory.");
                plan.repository = path;
            }
            let profiles = store.load_experts()?;
            let experts = plans
                .iter()
                .map(|p| {
                    if let Some(expert) = temporary
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
                "delegation_plan" => {
                    for expert in &temporary {
                        if !run.temporary_experts.iter().any(|e| e.profile.id == expert.profile.id) {
                            run.temporary_experts.push(expert.clone());
                        }
                    }
                    run.add_plans(caller, plans, &experts)?;
                },
                "delegation_message" => run.message(caller, uuid(args, "task_id")?, string(args, "message")?.into(), args.get("blocking").and_then(Value::as_bool).unwrap_or(false))?,
                "delegation_control" => run.control(caller, uuid(args,"task_id")?, string(args,"action")?, string(args,"reason")?)?,
                "delegation_complete" => {
                    let result: TaskResult = serde_json::from_value(args.get("result").cloned().context("Provide result.")?)?;
                    run.complete(caller, uuid(args,"task_id")?, uuid(args,"attempt_id")?, args.get("task_revision").and_then(Value::as_u64).context("Provide task_revision.")?, result)?;
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
            Ok(json!({"accepted":true,"run_id":run.id,"revision":run.revision + 1,"tasks":run.tasks.iter().map(|t|json!({"id":t.id,"key":t.plan.key,"status":t.status})).collect::<Vec<_>>(),"next":"End this turn when waiting, completing, or reporting completion. Choro delivers results at a safe boundary."}))
        })?;
        Ok(vec![text_content(serde_json::to_string(&response)?)])
    }
}
