use super::*;
use anyhow::Result;
use std::path::{Path, PathBuf};

pub fn conversation_path(design: Uuid, conversation: Uuid) -> PathBuf {
    PathBuf::from(format!(".choro/assistants/studio/{design}/{conversation}"))
}
pub fn context_from_path(path: &Path) -> Option<StudioAgentContext> {
    let parts: Vec<_> = path.to_str()?.split('/').collect();
    if parts.len() != 5
        || parts[..2] != [".choro", "assistants"]
        || !matches!(parts[2], "studio" | "studio-systems")
    {
        return None;
    }
    Some(StudioAgentContext {
        target: if parts[2] == "studio-systems" {
            StudioAgentTarget::DesignSystem
        } else {
            StudioAgentTarget::Design
        },
        design_id: parts[3].parse().ok()?,
        conversation_id: parts[4].parse().ok()?,
    })
}
pub fn system_prompt(context: &StudioAgentContext) -> String {
    if context.target == StudioAgentTarget::DesignSystem {
        return format!(
            "{}\n\nBound system: {}. Conversation: {}.\n\n{}",
            include_str!("../../assets/experts/skills/choro-studio/design-system.md"),
            context.design_id,
            context.conversation_id,
            include_str!("../../assets/experts/skills/frontend-design/SKILL.md")
        );
    }
    format!("{}\n\nBound design: {}. Conversation: {}.\n\nProject instructions and the Studio contract take priority over the following craft guidance.\n\n{}\n\n{}",
        include_str!("../../assets/experts/skills/choro-studio/SKILL.md"),context.design_id,context.conversation_id,
        include_str!("../../assets/experts/skills/frontend-design/SKILL.md"),
        include_str!("../../assets/experts/skills/web-design-guidelines/reference/guidelines.md"))
}
/// Authorize this design as the agent's workspace. The current screen/element
/// is a frozen conversational default, not a write-permission toggle. The agent
/// interprets the user's request; no keyword parser gates creation or chat.
pub fn scope_for_request(
    design: &StudioDesign,
    selected: Option<Uuid>,
    element: Option<String>,
) -> StudioTurnScope {
    StudioTurnScope {
        allow_shared_system: design.manifest.system_workspace,
        allow_create: !design.manifest.system_workspace,
        current_screen_id: selected,
        selected_element: element,
        base_revision: design.manifest.revision,
        base_fingerprint: design.fingerprint.clone(),
        ..StudioTurnScope::whole_design(design)
    }
}

/// Called on Stop, completion, failure, and backend teardown.
pub fn revoke_agent_scope(agent: &crate::AgentRecord) {
    if agent.studio_context.is_none() {
        return;
    }
    revoke_scope_at(&agent.project_path, agent.id);
}
pub fn revoke_scope_at(project: &Path, agent: Uuid) {
    if let Ok(store) = StudioStore::for_project(project) {
        if let Ok(mut scope) = store.scope(agent) {
            scope.active = false;
            let _ = store.save_scope(agent, &scope);
        }
    }
}

/// Inject host-frozen context into every actual provider request, including continuations.
pub fn attach_request_context(agent: &crate::AgentRecord, text: String) -> Result<String> {
    if agent.studio_context.is_none() {
        return Ok(text);
    }
    let store = StudioStore::for_project(&agent.project_path)?;
    let context = store.request_context(agent.id)?;
    Ok(format!(
        "<choro-studio-request-context>\n{}\n</choro-studio-request-context>\n\n{}",
        serde_json::to_string(&context)?,
        text
    ))
}
pub fn verify_agent_completion(agent: &crate::AgentRecord) -> Result<()> {
    if agent.studio_context.is_none() {
        return Ok(());
    }
    StudioStore::for_project(&agent.project_path)?.verify_turn_review(agent.id)
}
