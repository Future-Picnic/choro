//! Visual answers have their own tools and persistence; never use Preview APIs.
use super::*;
use ide_core::visual_answers::{VisualPresentInput, MAX_VISUAL_BYTES};

pub(super) fn tools() -> Vec<Box<dyn Tool>> {
    vec![Box::new(VisualPresentTool), Box::new(VisualReadTool)]
}

fn bound(ctx: &ServerContext) -> Result<(&LocalStore, uuid::Uuid, uuid::Uuid)> {
    anyhow::ensure!(
        ctx.review_run.is_none() && !studio_tools::is_studio(ctx),
        "Visual answers are unavailable in this restricted agent scope"
    );
    Ok((ctx.store()?, ctx.project_id()?.0, ctx.agent_id()?))
}

struct VisualPresentTool;
impl Tool for VisualPresentTool {
    fn name(&self) -> &'static str {
        "visual_present"
    }
    fn title(&self) -> &'static str {
        "Present visual answer"
    }
    fn description(&self) -> &'static str {
        "Publish or revise an OpenUI Visual answer in this agent's conversation. Informational tables, charts and summaries render in the conversation; only explicit choices in document.options attach to the composer. Local search, sorting and optional follow-up actions do not require a composer panel. Keep a short explanation in chat. Supply real data from your existing tools, source labels, units and time range; clearly label sample data. Use null for missing values. Reuse operation_key only for an identical retry; update with id and expected_revision from visual_read. This does not open, reload, register or replace project Preview and does not wait for a user response. The host decides when the answer may open. Never include credentials, executable queries, mutations or live network access."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type":"object","additionalProperties":false,
            "required":["operation_key","document"],
            "properties":{
                "operation_key":{"type":"string","minLength":1,"maxLength":200},
                "id":{"type":["string","null"],"format":"uuid"},
                "expected_revision":{"type":["integer","null"],"minimum":1},
                "document":{
                    "type":"object","additionalProperties":false,
                    "required":["title","summary","kind","openui_lang"],
                    "properties":{
                        "title":{"type":"string","minLength":1,"maxLength":200},
                        "summary":{"type":"string","minLength":1,"maxLength":8192},
                        "kind":{"enum":["table","line_chart","bar_chart","summary","design_options"]},
                        "openui_lang":{"type":"string","minLength":1,"maxLength":2097152},
                        "datasets":{"type":"object","maxProperties":16,"propertyNames":{"$ref":"#/$defs/identifier"},"additionalProperties":{"$ref":"#/$defs/dataset"}},
                        "sources":{"type":"array","maxItems":32,"items":{"$ref":"#/$defs/source"}},
                        "options":{"type":"array","maxItems":4,"items":{"$ref":"#/$defs/option"}},
                        "actions":{"type":"array","maxItems":16,"items":{"$ref":"#/$defs/action"}}
                    }
                }
            },
            "$defs":{
                "identifier":{"type":"string","minLength":1,"maxLength":128,"pattern":"^[A-Za-z0-9_-]+$"},
                "column":{"type":"object","additionalProperties":false,"required":["key","label","kind"],"properties":{
                    "key":{"$ref":"#/$defs/identifier"},"label":{"type":"string","minLength":1,"maxLength":200},
                    "kind":{"enum":["text","number","boolean","date"]},"unit":{"type":["string","null"],"maxLength":100}}},
                "dataset":{"type":"object","additionalProperties":false,"required":["columns","rows"],"properties":{
                    "columns":{"type":"array","minItems":1,"maxItems":64,"items":{"$ref":"#/$defs/column"}},
                    "rows":{"type":"array","maxItems":1000,"items":{"type":"object","additionalProperties":{"type":["string","number","boolean","null"]}}},
                    "series":{"type":"array","maxItems":8,"uniqueItems":true,"items":{"$ref":"#/$defs/identifier"},"description":"Numeric column keys. Declaring series limits this snapshot to 500 rows per series."}}},
                "source":{"type":"object","additionalProperties":false,"required":["label"],"properties":{
                    "label":{"type":"string","minLength":1,"maxLength":512},"reference":{"type":["string","null"],"maxLength":2048},
                    "retrieved_at":{"type":["integer","null"],"minimum":0},"time_range":{"type":["string","null"],"maxLength":512}}},
                "option":{"type":"object","additionalProperties":false,"required":["id","title","description","html"],"properties":{
                    "id":{"$ref":"#/$defs/identifier"},"title":{"type":"string","minLength":1,"maxLength":200},
                    "description":{"type":"string","maxLength":2048},"html":{"type":"string","minLength":1,"maxLength":262144,"description":"Self-contained HTML/CSS for a display-only sandbox; no scripts or external resources."}}},
                "action":{"type":"object","additionalProperties":false,"required":["id","label","kind"],"properties":{
                    "id":{"$ref":"#/$defs/identifier"},"label":{"type":"string","minLength":1,"maxLength":200},"kind":{"enum":["select","refine","follow_up"]}}}
            }
        })
    }
    fn call(&self, ctx: &ServerContext, args: &Value) -> Result<Vec<Value>> {
        anyhow::ensure!(
            serde_json::to_vec(args)?.len() <= MAX_VISUAL_BYTES,
            "Visual answer exceeds 2 MiB; reduce its scope or aggregate the data explicitly"
        );
        let input: VisualPresentInput = serde_json::from_value(args.clone())?;
        let (store, project, agent) = bound(ctx)?;
        let publication = store.present_visual_answer(project, agent, input)?;
        let placement = if publication.answer.document.requires_user_response() {
            "composer_visual_answer"
        } else {
            "agent_conversation_visual_answer"
        };
        Ok(vec![text_content(json!({"id":publication.answer.id,"revision":publication.answer.revision,"publication_status":"stored","duplicate":publication.duplicate,"summary":publication.answer.document.summary,"placement":placement,"preview_changed":false}).to_string())])
    }
}

struct VisualReadTool;
impl Tool for VisualReadTool {
    fn name(&self) -> &'static str {
        "visual_read"
    }
    fn title(&self) -> &'static str {
        "Read visual answer"
    }
    fn description(&self) -> &'static str {
        "Read a Visual answer's latest document revision and saved filters, selection, draft and submission receipt in this conversation. Optionally read an exact historical revision. Call before refining so expected_revision matches; preserve stable IDs where appropriate. A restored sending submission is uncertain and must never be automatically resent."
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","additionalProperties":false,"required":["id"],"properties":{"id":{"type":"string","format":"uuid"},"revision":{"type":"integer","minimum":1}}})
    }
    fn call(&self, ctx: &ServerContext, args: &Value) -> Result<Vec<Value>> {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Read {
            id: uuid::Uuid,
            revision: Option<u64>,
        }
        let args: Read = serde_json::from_value(args.clone())?;
        let (store, project, agent) = bound(ctx)?;
        let answer = store.read_visual_answer(project, agent, args.id, args.revision)?;
        Ok(vec![text_content(serde_json::to_string(&answer)?)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::{AgentAccessMode, AgentEffort, AgentKind, AgentModel, AgentRecord, AppConfig};
    fn fixture() -> (tempfile::TempDir, ServerContext, uuid::Uuid) {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
        let project = Project::from_path(dir.path().join("project"));
        let mut config = AppConfig::default();
        config.projects.push(project.clone());
        store.save_workspace_config(&config).unwrap();
        let agent = AgentRecord::new(
            project.id,
            project.path,
            "Visual",
            "Test",
            AgentKind::Codex,
            AgentModel::CodexDefault,
            AgentEffort::Medium,
            AgentAccessMode::FullAccess,
        );
        let other = AgentRecord {
            id: uuid::Uuid::new_v4(),
            ..agent.clone()
        };
        store.save_agents(&[agent.clone(), other.clone()]).unwrap();
        (
            dir,
            ServerContext {
                review_run: None,
                studio: false,
                delegation_scope: None,
                project_id: Some(project.id.0),
                agent_id: Some(agent.id),
                store: Some(store),
            },
            other.id,
        )
    }
    fn payload() -> Value {
        json!({"operation_key":"sample-table","document":{"title":"Sample projects","summary":"One explicitly labeled sample project.","kind":"table","openui_lang":"root = TextContent(\"Sample projects\")","datasets":{"projects":{"columns":[{"key":"name","label":"Project","kind":"text"}],"rows":[{"name":"Choro"}]}},"sources":[{"label":"Sample data"}]}})
    }
    #[test]
    fn tools_round_trip_and_enforce_conversation_scope_without_touching_preview() {
        let (_dir, mut ctx, other) = fixture();
        let result = VisualPresentTool.call(&ctx, &payload()).unwrap();
        let publication: Value = serde_json::from_str(result[0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(publication["publication_status"], "stored");
        assert_eq!(publication["placement"], "agent_conversation_visual_answer");
        let args = json!({"id":publication["id"]});
        let read = VisualReadTool.call(&ctx, &args).unwrap();
        let answer: Value = serde_json::from_str(read[0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(
            answer["document"]["datasets"]["projects"]["rows"][0]["name"],
            "Choro"
        );
        assert!(ctx
            .store()
            .unwrap()
            .load_project_previews(ide_core::ProjectId(ctx.project_id.unwrap()))
            .unwrap()
            .is_empty());
        ctx.agent_id = Some(other);
        assert!(VisualReadTool.call(&ctx, &args).is_err());
    }
    #[test]
    fn only_explicit_options_publish_as_composer_panels() {
        let (_dir, ctx, _) = fixture();
        let mut data = payload();
        data["document"]["actions"] = json!([
            {"id":"refine","label":"Show more","kind":"follow_up"}
        ]);
        let result = VisualPresentTool.call(&ctx, &data).unwrap();
        let info: Value = serde_json::from_str(result[0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(info["placement"], "agent_conversation_visual_answer");

        data["operation_key"] = json!("sample-designs");
        data["document"]["kind"] = json!("design_options");
        data["document"]["openui_lang"] =
            json!("root = Answer([choices])\nchoices = DesignOptions()");
        data["document"]["options"] = json!([
            {"id":"solid","title":"Solid","description":"","html":"<button>Solid</button>"},
            {"id":"outline","title":"Outline","description":"","html":"<button>Outline</button>"}
        ]);
        data["document"]["actions"] = json!([
            {"id":"use","label":"Use this style","kind":"select"}
        ]);
        let result = VisualPresentTool.call(&ctx, &data).unwrap();
        let choice: Value = serde_json::from_str(result[0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(choice["placement"], "composer_visual_answer");
        assert_eq!(choice["preview_changed"], false);
    }

    #[test]
    fn ownership_fields_and_unknown_arguments_cannot_be_model_supplied() {
        let (_dir, ctx, _) = fixture();
        let mut input = payload();
        input["agent_id"] = json!(ctx.agent_id);
        assert!(VisualPresentTool.call(&ctx, &input).is_err());
        let mut input = payload();
        input["document"]["datasets"]["projects"]["rows"][0]["name"] =
            json!({"unexpected":"nested"});
        assert!(VisualPresentTool.call(&ctx, &input).is_err());
        assert!(VisualReadTool
            .call(
                &ctx,
                &json!({"id":uuid::Uuid::new_v4(),"agent_id":ctx.agent_id})
            )
            .is_err());
    }
    #[test]
    fn tools_are_advertised_exactly_when_the_renderer_is_ready() {
        let tools = ToolRegistry::default().list();
        for name in ["visual_present", "visual_read"] {
            assert_eq!(
                tools.iter().any(|tool| tool["name"] == name),
                ide_core::visual_answers::VISUAL_ANSWERS_READY,
                "{name}"
            );
        }
    }
}
