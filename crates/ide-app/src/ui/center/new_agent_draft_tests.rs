//! In-memory GPUI fixtures; these never open or modify the user's workspace.
use super::*;

struct Fixture;

impl Render for Fixture {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

fn draft(project: ProjectId, text: &str, window: &mut Window, cx: &mut App) -> NewAgentComposer {
    let prompt = cx.new(|cx| InputState::new(window, cx).default_value(text.to_owned()));
    NewAgentComposer::new(
        project,
        None,
        prompt,
        ide_core::config::NewAgentDefaults::for_provider(AgentKind::Codex),
    )
}

#[gpui::test]
fn new_agent_draft_restores_text_images_and_settings_per_project(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_component::init);
    cx.add_window_view(|window, cx| {
        let first = ProjectId(Uuid::new_v4());
        let second = ProjectId(Uuid::new_v4());
        let mut composer = draft(first, "Fix this screenshot", window, cx);
        let id = composer.id;
        let input_id = composer.prompt.entity_id();
        composer
            .attached_files
            .push(PathBuf::from("/draft/screenshot.png"));
        composer
            .linked_docs
            .push(PathBuf::from("choro_docs/plan.choro"));
        composer.solo = true;
        composer.interaction_mode = AgentInteractionMode::Plan;
        let mut active = Some(composer);
        let mut drafts = HashMap::new();
        stash_new_agent_draft(&mut active, &mut drafts);
        assert!(active.is_none());
        active = Some(draft(second, "Another project", window, cx));
        stash_new_agent_draft(&mut active, &mut drafts);
        active = drafts.remove(&first);
        let restored = active.as_ref().unwrap();
        assert_eq!(restored.id, id);
        assert_eq!(restored.prompt.entity_id(), input_id);
        assert_eq!(
            restored.prompt.read(cx).value().as_str(),
            "Fix this screenshot"
        );
        assert_eq!(
            restored.attached_files,
            [PathBuf::from("/draft/screenshot.png")]
        );
        assert_eq!(
            restored.linked_docs,
            [PathBuf::from("choro_docs/plan.choro")]
        );
        assert!(restored.solo);
        assert_eq!(restored.interaction_mode, AgentInteractionMode::Plan);
        assert_eq!(
            drafts[&second].prompt.read(cx).value().as_str(),
            "Another project"
        );
        Fixture
    });
}

#[gpui::test]
fn new_agent_draft_accepts_background_completion_while_away(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_component::init);
    cx.add_window_view(|window, cx| {
        let project = ProjectId(Uuid::new_v4());
        let other = ProjectId(Uuid::new_v4());
        let mut composer = draft(project, "Image still pasting", window, cx);
        let id = composer.id;
        composer.attachment_pastes_pending = 1;
        composer.starting = true;
        let mut active = Some(composer);
        let mut drafts = HashMap::new();
        stash_new_agent_draft(&mut active, &mut drafts);
        active = Some(draft(other, "Do not change this draft", window, cx));
        let pending = find_new_agent_draft(&mut active, &mut drafts, project, id).unwrap();
        pending.attachment_pastes_pending -= 1;
        pending
            .attached_files
            .push(PathBuf::from("/draft/finished.png"));
        pending.starting = false;
        pending.error = Some("Save failed; retry".into());
        assert!(active.as_ref().unwrap().attached_files.is_empty());
        assert!(find_new_agent_draft(&mut active, &mut drafts, project, Uuid::new_v4()).is_none());
        stash_new_agent_draft(&mut active, &mut drafts);
        let restored = drafts.remove(&project).unwrap();
        assert_eq!(restored.attachment_pastes_pending, 0);
        assert_eq!(
            restored.attached_files,
            [PathBuf::from("/draft/finished.png")]
        );
        assert!(
            !restored.starting,
            "a failed launch must remain retryable after returning"
        );
        assert_eq!(restored.error.as_deref(), Some("Save failed; retry"));
        Fixture
    });
}

#[gpui::test]
fn new_agent_draft_success_clears_only_the_submitted_composer(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_component::init);
    cx.add_window_view(|window, cx| {
        let project = ProjectId(Uuid::new_v4());
        let mut active = Some(draft(project, "Submitted", window, cx));
        let id = active.as_ref().unwrap().id;
        let mut drafts = HashMap::new();
        stash_new_agent_draft(&mut active, &mut drafts);
        active = Some(draft(project, "Newer unsent work", window, cx));
        clear_new_agent_draft(&mut active, &mut drafts, project, id);
        assert!(!drafts.contains_key(&project));
        assert_eq!(
            active.as_ref().unwrap().prompt.read(cx).value().as_str(),
            "Newer unsent work"
        );
        let newer_id = active.as_ref().unwrap().id;
        clear_new_agent_draft(&mut active, &mut drafts, project, newer_id);
        assert!(active.is_none());
        Fixture
    });
}
