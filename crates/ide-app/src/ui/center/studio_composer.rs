use super::*;
use crate::state::agent_chat::StudioChatRequest;
use ide_core::studio::{StudioAgentTarget, StudioStore};

impl CenterArea {
    pub(super) fn capture_studio_chat_request(&self, agent: &AgentRecord, cx: &App) -> anyhow::Result<Option<StudioChatRequest>> {
        let Some(context) = &agent.studio_context else { return Ok(None); };
        if context.target == StudioAgentTarget::DesignSystemImport {
            let store = StudioStore::for_project(&agent.project_path)?;
            return Ok(Some(StudioChatRequest {
                context: context.clone(), project: store.project, cache: store.cache,
                current_screen_id: None, current_section_id: None, selected_element: None, design_guidance: String::new(),
                target_label: "From code".into(),
            }));
        }
        let studio = self.studio.as_ref().filter(|s| s.design.manifest.id == context.design_id)
            .ok_or_else(|| anyhow::anyhow!("Open this Studio design before sending a request."))?;
        anyhow::ensure!(!studio.dirty && !studio.saving, "Wait for the current screen to save before sending.");
        let screen_id = studio.editing_screen().or(studio.canvas.layout.selected_screen_id);
        let section = studio.canvas.layout.selected_section_id
            .filter(|_| screen_id.is_none() && context.target == StudioAgentTarget::Design && !studio.design.manifest.system_workspace)
            .and_then(|id| studio.design.manifest.section(id));
        let target_label = if context.target == StudioAgentTarget::DesignSystem {
            "Design system".into()
        } else if let Some(section) = section {
            format!("Section · {}", section.name)
        } else {
            screen_id.and_then(|id| studio.design.manifest.screens.iter().find(|s| s.id == id && !s.archived))
                .map(|s| s.name.clone()).unwrap_or_else(|| "All screens".into())
        };
        Ok(Some(StudioChatRequest {
            context: context.clone(), project: studio.store.project.clone(), cache: studio.store.cache.clone(),
            current_screen_id: screen_id, current_section_id: section.map(|s| s.id),
            selected_element: screen_id.and(studio.selected_element.clone()),
            design_guidance: self.workspace.read(cx).studio.request_guidance(), target_label,
        }))
    }

    pub(super) fn render_studio_composer_target(&self, agent: &AgentRecord, cx: &App) -> Option<gpui::AnyElement> {
        let context = agent.studio_context.as_ref()?;
        if context.target != StudioAgentTarget::Design { return None; }
        let studio = self.studio.as_ref().filter(|s| s.design.manifest.id == context.design_id)?;
        let screen = studio.editing_screen().or(studio.canvas.layout.selected_screen_id)
            .and_then(|id| studio.design.manifest.screens.iter().find(|s| s.id == id && !s.archived));
        let section = studio.canvas.layout.selected_section_id.filter(|_| screen.is_none() && !studio.design.manifest.system_workspace)
            .and_then(|id| studio.design.manifest.section(id));
        let label = if let Some(section) = section { format!("Section · {}", section.name) }
            else { screen.map(|s| s.name.clone()).unwrap_or_else(|| "All screens".into()) };
        let tooltip = format!("{} · Default target for your next message. Queued messages keep the target chosen when sent.", label);
        Some(crate::ui::style::composer_target_chip(label, screen.is_some(), cx)
            .id("studio-composer-target")
            .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx)).into_any_element())
    }
}
