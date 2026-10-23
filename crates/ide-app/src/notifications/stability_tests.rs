use super::*;

fn event(parent: Uuid, source: Uuid, revision: &str) -> AttentionEvent {
    AttentionEvent::new(
        ProjectId(Uuid::nil()),
        parent,
        AttentionCategory::NeedsAction,
        revision,
        1,
        "Chat",
        "Project",
    )
    .with_source(source)
}

#[test]
fn acknowledged_revision_survives_restart_and_temporary_absence() {
    let id = Uuid::new_v4();
    let mut old = event(id, id, "old-response");
    old.key.category = AttentionCategory::Completed;
    let mut state = NotificationCoordinator::default();
    state.synchronize(
        vec![old.clone()],
        None,
        false,
        NotificationSettings::default(),
    );
    state.acknowledge_agent(ProjectId(Uuid::nil()), id);
    state.synchronize(vec![], None, false, NotificationSettings::default());
    state.synchronize(
        vec![old.clone()],
        None,
        false,
        NotificationSettings::default(),
    );
    assert!(state.companion_attention().is_empty());
    assert_eq!(state.unread_agent_count(), 0);
    let encoded = serde_json::to_vec(&state.acknowledgement_order).unwrap();
    let restored: Vec<(AttentionKey, String)> = serde_json::from_slice(&encoded).unwrap();
    let mut restarted = NotificationCoordinator::default();
    for entry in restored {
        restarted.remember(entry);
    }
    restarted.synchronize(
        vec![old.clone()],
        None,
        false,
        NotificationSettings::default(),
    );
    assert!(restarted.companion_attention().is_empty());
    old.revision = "new-response".into();
    restarted.synchronize(vec![old], None, false, NotificationSettings::default());
    assert_eq!(restarted.companion_attention().len(), 1);
    assert_eq!(restarted.unread_agent_count(), 1);
}

#[test]
fn two_bandmates_keep_separate_revisions_and_route_to_their_lead() {
    let parent = Uuid::new_v4();
    let first = event(parent, Uuid::new_v4(), "question-1");
    let mut second = event(parent, Uuid::new_v4(), "question-2");
    assert_ne!(
        notification_identifier(&first),
        notification_identifier(&second)
    );
    let route = parse_notification_route(&notification_identifier(&first)).unwrap();
    assert_eq!(route.agent_id, parent);
    let mut state = NotificationCoordinator::default();
    state.synchronize(
        vec![first.clone(), second.clone()],
        None,
        false,
        NotificationSettings::default(),
    );
    assert_eq!(state.companion_attention().len(), 1);
    state.acknowledge_agent(ProjectId(Uuid::nil()), parent);
    for _ in 0..3 {
        state.synchronize(
            vec![second.clone(), first.clone()],
            None,
            false,
            NotificationSettings::default(),
        );
        assert_eq!(state.unread_agent_count(), 0);
        assert!(state.companion_attention().is_empty());
    }
    second.revision = "question-3".into();
    state.synchronize(
        vec![first, second],
        None,
        false,
        NotificationSettings::default(),
    );
    assert_eq!(state.companion_attention().len(), 1);
}
