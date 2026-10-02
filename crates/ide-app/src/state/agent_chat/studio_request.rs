//! Frozen composer targets. Queueing never changes the active Studio scope.
use anyhow::{ensure, Result};
use ide_core::studio::*;
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StudioChatRequest {
    pub context: StudioAgentContext,
    pub project: PathBuf,
    pub cache: PathBuf,
    pub current_screen_id: Option<Uuid>,
    pub current_section_id: Option<Uuid>,
    pub selected_element: Option<String>,
    pub design_guidance: String,
    pub target_label: String,
}

impl StudioChatRequest {
    pub fn title_conversation(&self, text: &str) -> Result<()> {
        if self.context.target != StudioAgentTarget::DesignSystemImport {
            let store = StudioStore { project: self.project.clone(), cache: self.cache.clone() };
            store.title_conversation(self.context.design_id, self.context.conversation_id, text)?;
        }
        Ok(())
    }

    /// Check a pending target without replacing a running turn's scope.
    pub fn validate(&self) -> Result<()> {
        if self.context.target == StudioAgentTarget::DesignSystemImport {
            return Ok(());
        }
        let store = StudioStore { project: self.project.clone(), cache: self.cache.clone() };
        let design = store.load(self.context.design_id)?;
        self.validate_target(&design)
    }

    fn validate_target(&self, design: &StudioDesign) -> Result<()> {
        ensure!(self.current_screen_id.is_none() || self.current_section_id.is_none(),
            "Choose a screen or section as the request target.");
        if let Some(id) = self.current_screen_id {
            ensure!(design.manifest.screens.iter().any(|s| s.id == id && !s.archived),
                "The queued screen is no longer available. Edit the queued message to choose another target.");
        }
        if let Some(id) = self.current_section_id {
            ensure!(self.context.target == StudioAgentTarget::Design && design.manifest.section(id).is_some(),
                "The queued section is no longer available. Edit the queued message to choose another target.");
        }
        Ok(())
    }

    /// Called only when a turn starts, using the latest saved design revision.
    pub fn activate(&self, agent_id: Uuid, text: &str) -> Result<()> {
        let store = StudioStore { project: self.project.clone(), cache: self.cache.clone() };
        if self.context.target == StudioAgentTarget::DesignSystemImport {
            store.prepare_code_import(agent_id, &self.context)?;
            return Ok(());
        }
        let design = store.load(self.context.design_id)?;
        self.validate_target(&design)?;
        let mut scope = match self.current_section_id {
            Some(id) => scope_for_section_request(&design, id),
            None => scope_for_request(&design, self.current_screen_id, self.selected_element.clone()),
        };
        if self.context.target == StudioAgentTarget::Design {
            scope.design_guidance = self.design_guidance.clone();
        }
        store.title_conversation(self.context.design_id, self.context.conversation_id, text)?;
        atomic(&store.cache.join("roles").join(format!("{agent_id}.json")), &serde_json::to_vec(&self.context)?)?;
        store.save_scope(agent_id, &scope)?;
        Ok(())
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    pub(crate) fn fixture() -> (StudioStore, StudioDesign, Uuid, StudioChatRequest) {
        let root = std::env::temp_dir().join(format!("choro-studio-queue-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("project")).unwrap();
        let store = StudioStore::new(root.join("project"), root.join("data")).unwrap();
        let design = store.create("Queued designer requests").unwrap();
        let conversation = store.conversations(design.manifest.id).unwrap().selected;
        let agent = Uuid::new_v4();
        let request = StudioChatRequest {
            context: StudioAgentContext {
                target: StudioAgentTarget::Design, design_id: design.manifest.id,
                conversation_id: conversation,
            },
            project: store.project.clone(), cache: store.cache.clone(),
            current_screen_id: Some(design.manifest.screens[0].id),
            current_section_id: None,
            selected_element: Some("heading".into()), design_guidance: "Preserve the product theme".into(),
            target_label: design.manifest.screens[0].name.clone(),
        };
        (store, design, agent, request)
    }

    #[test]
    fn studio_queued_request_activates_latest_revision_with_frozen_target() {
        let (store, design, agent, request) = fixture();
        let running = scope_for_request(&design, None, None);
        store.save_scope(agent, &running).unwrap();
        let mut screen = design.manifest.screens[0].clone();
        screen.name = "Renamed while request was queued".into();
        let newer = store.apply(&running, &StudioTransaction {
            id: Uuid::new_v4(), scope_id: running.id, design_id: design.manifest.id,
            expected_revision: design.manifest.revision, expected_fingerprint: design.fingerprint.clone(),
            operations: vec![StudioOperation::UpdateScreen { screen }],
        }).unwrap();
        request.validate().unwrap();
        assert_eq!(store.scope(agent).unwrap().id, running.id);
        // The preceding backend may revoke its scope during shutdown. A
        // pending request activates only after that cleanup has completed.
        let mut ended = running.clone();
        ended.active = false;
        store.save_scope(agent, &ended).unwrap();
        request.activate(agent, "Make this heading clearer").unwrap();
        let active = store.scope(agent).unwrap();
        assert_ne!(active.id, running.id);
        assert_eq!(active.base_revision, newer.manifest.revision);
        assert_eq!(active.base_fingerprint, newer.fingerprint);
        assert_eq!(active.current_screen_id, request.current_screen_id);
        assert_eq!(active.selected_element, request.selected_element);
        assert_eq!(active.design_guidance, request.design_guidance);
        assert!(active.allow_create && active.allow_design_metadata);
        let frozen = store.request_context(agent).unwrap();
        assert_eq!(frozen["scope"]["current_screen_id"], serde_json::json!(request.current_screen_id));
    }

    #[test]
    fn studio_archived_queued_target_preserves_previous_scope() {
        let (store, design, agent, request) = fixture();
        let running = scope_for_request(&design, None, None);
        store.save_scope(agent, &running).unwrap();
        let mut screen = design.manifest.screens[0].clone();
        screen.archived = true;
        store.apply(&running, &StudioTransaction {
            id: Uuid::new_v4(), scope_id: running.id, design_id: design.manifest.id,
            expected_revision: design.manifest.revision, expected_fingerprint: design.fingerprint,
            operations: vec![StudioOperation::UpdateScreen { screen }],
        }).unwrap();
        assert!(request.activate(agent, "Edit the selected screen").is_err());
        assert_eq!(store.scope(agent).unwrap().id, running.id);
    }

    #[test]
    fn studio_queued_section_uses_latest_revision_and_clears_screen_defaults() {
        let (store, design, agent, mut request) = fixture();
        let section = StudioSection { screen_ids: vec![design.manifest.screens[0].id], ..StudioSection::new("Add post") };
        let scope = StudioTurnScope::whole_design(&design);
        let grouped = store.apply(&scope, &StudioTransaction {
            id: Uuid::new_v4(), scope_id: scope.id, design_id: design.manifest.id,
            expected_revision: design.manifest.revision, expected_fingerprint: design.fingerprint,
            operations: vec![StudioOperation::CreateSection { section: section.clone() }],
        }).unwrap();
        let running = scope_for_request(&grouped, request.current_screen_id, request.selected_element.clone());
        store.save_scope(agent, &running).unwrap();
        request.current_screen_id = None;
        request.current_section_id = Some(section.id);
        let newer = store.apply(&running, &StudioTransaction {
            id: Uuid::new_v4(), scope_id: running.id, design_id: grouped.manifest.id,
            expected_revision: grouped.manifest.revision, expected_fingerprint: grouped.fingerprint,
            operations: vec![StudioOperation::UpdateSection {
                section_id: section.id, name: Some("Add post flow".into()), direction: None,
                gap: None, title_style: None, header_alignment: None,
            }],
        }).unwrap();
        request.validate().unwrap();
        assert_eq!(store.scope(agent).unwrap().id, running.id);
        request.activate(agent, "Edit this flow").unwrap();
        let active = store.scope(agent).unwrap();
        assert_eq!(active.current_section_id, Some(section.id));
        assert_eq!(active.current_screen_id, None);
        assert_eq!(active.selected_element, None);
        assert_eq!(active.base_revision, newer.manifest.revision);
        assert_eq!(active.base_fingerprint, newer.fingerprint);
        let frozen = store.request_context(agent).unwrap();
        assert_eq!(frozen["current_section"]["name"], "Add post flow");
        assert_eq!(frozen["current_section"]["screen_ids"], serde_json::json!(section.screen_ids));
        assert!(active.allow_create && active.allow_design_metadata);
        request.current_section_id = None;
        request.current_screen_id = Some(grouped.manifest.screens[0].id);
        request.activate(agent, "Edit this screen").unwrap();
        assert_eq!(store.scope(agent).unwrap().current_section_id, None);
    }

    #[test]
    fn studio_removed_queued_section_preserves_scope_and_does_not_target_all_screens() {
        let (store, design, agent, mut request) = fixture();
        let section = StudioSection::new("Add images");
        let scope = StudioTurnScope::whole_design(&design);
        let grouped = store.apply(&scope, &StudioTransaction {
            id: Uuid::new_v4(), scope_id: scope.id, design_id: design.manifest.id,
            expected_revision: design.manifest.revision, expected_fingerprint: design.fingerprint,
            operations: vec![StudioOperation::CreateSection { section: section.clone() }],
        }).unwrap();
        let running = scope_for_request(&grouped, request.current_screen_id, None);
        store.save_scope(agent, &running).unwrap();
        request.current_screen_id = None;
        request.current_section_id = Some(section.id);
        store.apply(&running, &StudioTransaction {
            id: Uuid::new_v4(), scope_id: running.id, design_id: grouped.manifest.id,
            expected_revision: grouped.manifest.revision, expected_fingerprint: grouped.fingerprint,
            operations: vec![StudioOperation::UngroupSection { section_id: section.id }],
        }).unwrap();
        assert!(request.validate().is_err());
        assert!(request.activate(agent, "Edit this flow").is_err());
        assert_eq!(store.scope(agent).unwrap().id, running.id);
    }

    #[test]
    fn studio_all_screens_request_drops_previous_screen_default() {
        let (store, _, agent, mut request) = fixture();
        request.activate(agent, "Change the heading").unwrap();
        request.current_screen_id = None;
        request.selected_element = None;
        request.activate(agent, "Review all screens").unwrap();
        let active = store.scope(agent).unwrap();
        assert_eq!(active.current_screen_id, None);
        assert_eq!(active.selected_element, None);
        assert!(active.allow_create && active.allow_design_metadata);
    }
}
