use super::*;
#[test]
fn new_screens_and_neighbor_edits_rebase_without_losing_manual_changes() {
    let (_directory, store, design) = fixture();
    let scope = StudioTurnScope::whole_design(&design);
    let manual = edit(&design, &scope);
    let changed = store.apply(&scope, &manual).unwrap();
    let new_id = Uuid::new_v4();
    let mut create = edit(&design, &scope);
    create.operations = vec![StudioOperation::CreateScreen {
        screen: StudioScreen {
            id: new_id,
            name: "Settings".into(),
            ..design.manifest.screens[0].clone()
        },
        document: StudioDocument {
            html: "<!doctype html><html><body></body></html>".into(),
            ..Default::default()
        },
    }];
    let created = store.apply(&scope, &create).unwrap();
    assert_eq!(
        created.documents[&design.manifest.screens[0].id],
        changed.documents[&design.manifest.screens[0].id]
    );
    assert_eq!(store.apply(&scope, &create).unwrap(), created);
    let expanded_scope = StudioTurnScope::whole_design(&created);
    let home_edit = edit(&created, &expanded_scope);
    // Make a real additional Home edit while the agent builds Settings.
    let mut home_edit = home_edit;
    if let StudioOperation::WriteScreen { document, .. } = &mut home_edit.operations[0] {
        document.html = "<h1>My manual heading</h1>".into();
    }
    store.apply(&expanded_scope, &home_edit).unwrap();
    let mut settings_edit = edit(&created, &expanded_scope);
    settings_edit.operations = vec![StudioOperation::WriteScreen {
        screen_id: new_id,
        document: StudioDocument {
            html: "<h1>Settings</h1>".into(),
            ..Default::default()
        },
    }];
    let merged = store.apply(&expanded_scope, &settings_edit).unwrap();
    assert_eq!(merged.documents[&new_id].html, "<h1>Settings</h1>");
    assert_eq!(
        merged.documents[&design.manifest.screens[0].id].html,
        "<h1>My manual heading</h1>"
    );
    // A competing edit to that same screen must still conflict and preserve both proposals.
    let overlapping = edit(&created, &expanded_scope);
    assert!(store.apply(&expanded_scope, &overlapping).is_err());
    assert!(store
        .cache
        .join("conflicts")
        .join(format!("{}.json", overlapping.id))
        .exists());
    assert_eq!(store.load(design.manifest.id).unwrap(), merged);
}
#[test]
fn stale_screen_edits_cannot_bypass_scope_or_changed_tokens() {
    let (_directory, store, design) = fixture();
    let scope = StudioTurnScope::whole_design(&design);
    let mut tokens = edit(&design, &scope);
    let mut overrides = design.overrides.clone();
    overrides
        .tokens
        .insert("color-primary".into(), "#abcdef".into());
    tokens.operations = vec![StudioOperation::SetOverrides { overrides }];
    store.apply(&scope, &tokens).unwrap();
    assert!(store.apply(&scope, &edit(&design, &scope)).is_err());
    let latest = store.load(design.manifest.id).unwrap();
    let mut no_scope = StudioTurnScope::screen(design.manifest.id, Uuid::new_v4());
    no_scope.id = scope.id;
    let mut tx = edit(&latest, &no_scope);
    tx.expected_revision = design.manifest.revision;
    tx.expected_fingerprint = design.fingerprint.clone();
    assert!(store.apply(&no_scope, &tx).is_err());
}
#[test]
fn implementation_links_follow_snapshot_design_and_persist() {
    let (_directory, store, design) = fixture();
    let other = store.create("Other").unwrap();
    let snapshot = store.handoff(design.manifest.id, None).unwrap();
    let agent = Uuid::new_v4();
    store.link_implementation_agent(snapshot.id, agent).unwrap();
    store.link_implementation_agent(snapshot.id, agent).unwrap();
    let reopened = StudioStore {
        project: store.project.clone(),
        cache: store.cache.clone(),
    };
    assert_eq!(
        reopened.implementation_agents(design.manifest.id).unwrap(),
        vec![agent]
    );
    assert!(reopened
        .implementation_agents(other.manifest.id)
        .unwrap()
        .is_empty());
    assert!(reopened
        .link_implementation_agent(Uuid::new_v4(), Uuid::new_v4())
        .is_err());
    assert_eq!(
        reopened.implementation_agents(design.manifest.id).unwrap(),
        vec![agent]
    );
}
#[test]
fn conversation_history_preserves_legacy_identity_and_survives_reopen() {
    let (_directory, store, design) = fixture();
    let id = design.manifest.id;
    let legacy = Uuid::new_v4();
    atomic(
        &store.cache.join(format!("conversation-{id}.json")),
        &serde_json::to_vec(&legacy).unwrap(),
    )
    .unwrap();
    assert_eq!(store.conversations(id).unwrap().selected, legacy);
    let titled = store
        .title_conversation(id, legacy, "  Improve\n the login screen ")
        .unwrap();
    assert_eq!(titled.entries[0].title, "Improve the login screen");
    let fresh = store.select_conversation(id, None).unwrap();
    assert_ne!(fresh.selected, legacy);
    assert_eq!(fresh.entries.len(), 2);
    let reopened = StudioStore {
        project: store.project.clone(),
        cache: store.cache.clone(),
    };
    assert_eq!(reopened.conversations(id).unwrap(), fresh);
    reopened.select_conversation(id, Some(legacy)).unwrap();
    let history = reopened
        .title_conversation(id, legacy, "A later request")
        .unwrap();
    assert_eq!(history.selected, legacy);
    assert_eq!(history.entries[0].title, "Improve the login screen");
    assert_eq!(history.entries.len(), 2);
    assert!(store.cache.join(format!("conversation-{id}.json")).exists());
}
#[test]
fn conversation_selection_is_isolated_per_design() {
    let (_directory, store, design) = fixture();
    let other = store.create("Other design").unwrap();
    let first = store.conversations(design.manifest.id).unwrap();
    let second = store.conversations(other.manifest.id).unwrap();
    assert_ne!(first.selected, second.selected);
    assert!(store
        .select_conversation(design.manifest.id, Some(second.selected))
        .is_err());
    assert!(store
        .title_conversation(design.manifest.id, second.selected, "Foreign prompt")
        .is_err());
    assert_eq!(store.conversations(design.manifest.id).unwrap(), first);
    assert_eq!(store.conversations(other.manifest.id).unwrap(), second);
    assert_ne!(
        conversation_path(first.design_id, first.selected),
        conversation_path(second.design_id, second.selected)
    );
}
fn fixture() -> (tempfile::TempDir, StudioStore, StudioDesign) {
    let directory = tempfile::tempdir().unwrap();
    let project = directory.path().join("project");
    fs::create_dir(&project).unwrap();
    let store = StudioStore::new(project, directory.path().join("data")).unwrap();
    let design = store.create("Example").unwrap();
    (directory, store, design)
}
fn edit(design: &StudioDesign, scope: &StudioTurnScope) -> StudioTransaction {
    StudioTransaction {
        id: Uuid::new_v4(),
        scope_id: scope.id,
        design_id: design.manifest.id,
        expected_revision: design.manifest.revision,
        expected_fingerprint: design.fingerprint.clone(),
        operations: vec![StudioOperation::WriteScreen {
            screen_id: design.manifest.screens[0].id,
            document: StudioDocument {
                html: "<h1>Changed</h1>".into(),
                ..Default::default()
            },
        }],
    }
}
#[test]
fn revision_conflicts_and_idempotency_preserve_latest_edits() {
    let (_directory, store, design) = fixture();
    let scope = StudioTurnScope::whole_design(&design);
    let transaction = edit(&design, &scope);
    let changed = store.apply(&scope, &transaction).unwrap();
    assert_eq!(changed.manifest.revision, 1);
    assert_eq!(store.apply(&scope, &transaction).unwrap(), changed);
    assert!(store
        .apply(&scope, &edit(&design, &scope))
        .unwrap_err()
        .to_string()
        .contains("conflict"));
    assert_eq!(store.transaction_before(transaction.id).unwrap(), design);
}
#[test]
fn direct_scope_escalation_is_rejected() {
    let (_directory, store, design) = fixture();
    let scope = StudioTurnScope::screen(design.manifest.id, Uuid::new_v4());
    assert!(store.apply(&scope, &edit(&design, &scope)).is_err());
    let scope = StudioTurnScope::screen(design.manifest.id, design.manifest.screens[0].id);
    let mut transaction = edit(&design, &scope);
    transaction.operations = vec![StudioOperation::RenameDesign {
        name: "Unexpected title".into(),
    }];
    assert!(store.apply(&scope, &transaction).is_err());
    transaction.operations = vec![StudioOperation::SetSystem {
        system: design.system.clone(),
        expected_system_revision: 0,
    }];
    assert!(store.apply(&scope, &transaction).is_err());
    transaction.operations = vec![StudioOperation::CreateScreen {
        screen: StudioScreen {
            id: Uuid::new_v4(),
            ..design.manifest.screens[0].clone()
        },
        document: starter_document(),
    }];
    assert!(store.apply(&scope, &transaction).is_err());
    transaction.design_id = Uuid::new_v4();
    assert!(store.apply(&scope, &transaction).is_err());
    assert_eq!(store.load(design.manifest.id).unwrap(), design);
}
#[test]
fn external_file_edits_are_not_overwritten() {
    let (_directory, store, design) = fixture();
    let scope = StudioTurnScope::whole_design(&design);
    let path = store.project.join(format!(
        "{DESIGNS_DIR}/{}/screens/{}/index.html",
        design.manifest.id, design.manifest.screens[0].id
    ));
    fs::write(&path, "<h1>External</h1>").unwrap();
    assert!(store.apply(&scope, &edit(&design, &scope)).is_err());
    assert_eq!(fs::read_to_string(path).unwrap(), "<h1>External</h1>");
}
#[test]
fn handoff_survives_source_changes_and_includes_untracked_assets() {
    let (_directory, store, design) = fixture();
    let assets = store
        .project
        .join(format!("{DESIGNS_DIR}/{}/assets", design.manifest.id));
    fs::create_dir_all(&assets).unwrap();
    fs::write(assets.join("example.txt"), "sample").unwrap();
    let design = store.load(design.manifest.id).unwrap();
    let snapshot = store.handoff(design.manifest.id, None).unwrap();
    let scope = StudioTurnScope::whole_design(&design);
    store.apply(&scope, &edit(&design, &scope)).unwrap();
    let loaded = store.read_handoff(snapshot.id).unwrap();
    assert_eq!(loaded.design, snapshot.design);
    assert_eq!(loaded.assets["assets/example.txt"], b"sample");
}
#[test]
fn overrides_inherit_and_shared_updates_invalidate_other_designs() {
    let (_directory, store, design) = fixture();
    let record = store.create_system("Web", "Web", None).unwrap();
    let system = system_edit(&store, record.id, StudioDesignSystem::default());
    store
        .publish_system(
            record.id,
            system.manifest.revision,
            &store.load(record.id).unwrap().fingerprint,
            &[],
        )
        .unwrap();
    let design = bind_system(&store, &design, Some(record.id));
    let other = bind_system(&store, &store.create("Another").unwrap(), Some(record.id));
    let scope = StudioTurnScope::whole_design(&design);
    let mut transaction = edit(&design, &scope);
    transaction.operations = vec![StudioOperation::SetOverrides {
        overrides: StudioOverrides {
            screen_styles: Default::default(),
            tokens: [("color-primary".into(), "#abcdef".into())].into(),
        },
    }];
    let changed = store.apply(&scope, &transaction).unwrap();
    let mut next = system.system;
    next.tokens.insert("color-primary".into(), "#123456".into());
    let system = system_edit(&store, record.id, next);
    assert_eq!(
        store.load(other.manifest.id).unwrap(),
        other,
        "drafts must not change linked designs"
    );
    let impacts = store.system_impact(record.id).unwrap();
    store
        .publish_system(
            record.id,
            system.manifest.revision,
            &store.load(record.id).unwrap().fingerprint,
            &impacts,
        )
        .unwrap();
    assert_eq!(
        store.load(changed.manifest.id).unwrap().tokens()["color-primary"],
        "#abcdef"
    );
    assert_ne!(
        other.fingerprint,
        store.load(other.manifest.id).unwrap().fingerprint
    );
}

#[test]
#[cfg(unix)]
fn traversal_and_symlinks_are_rejected() {
    let (directory, store, design) = fixture();
    assert!(contained(&store.project, "../outside").is_err());
    assert!(contained(&store.project, "/absolute").is_err());
    let outside = directory.path().join("outside");
    fs::create_dir(&outside).unwrap();
    std::os::unix::fs::symlink(
        &outside,
        store
            .project
            .join(format!("{DESIGNS_DIR}/{}/assets", design.manifest.id)),
    )
    .unwrap();
    assert!(store.load(design.manifest.id).is_err());
}
#[test]
fn ended_turn_and_reused_operation_key_are_rejected() {
    let (_directory, store, design) = fixture();
    let mut scope = StudioTurnScope::whole_design(&design);
    let mut tx = edit(&design, &scope);
    store.apply(&scope, &tx).unwrap();
    tx.operations = vec![StudioOperation::RenameDesign {
        name: "Other".into(),
    }];
    assert!(store.apply(&scope, &tx).is_err());
    scope.active = false;
    assert!(store.apply(&scope, &tx).is_err());
}

#[test]
fn thumbnail_changes_are_scoped_to_the_changed_screen() {
    let (_directory, store, design) = fixture();
    let scope = StudioTurnScope::whole_design(&design);
    let second = StudioScreen {
        id: Uuid::new_v4(),
        name: "Second".into(),
        width: 800,
        height: 600,
        archived: false,
        files: StudioScreenFiles::default(),
    };
    let mut tx = edit(&design, &scope);
    tx.operations = vec![StudioOperation::CreateScreen {
        screen: second.clone(),
        document: starter_document(),
    }];
    let design = store.apply(&scope, &tx).unwrap();
    let original = store.thumbnail_path(&design, second.id);
    let updated = store.apply(&scope, &edit(&design, &scope)).unwrap();
    assert_eq!(original, store.thumbnail_path(&updated, second.id));
    assert_ne!(
        store.thumbnail_path(&design, design.manifest.screens[0].id),
        store.thumbnail_path(&updated, design.manifest.screens[0].id)
    );
}
#[test]
fn host_revocation_is_checked_again_at_commit() {
    let (_directory, store, design) = fixture();
    let agent = Uuid::new_v4();
    let mut scope = StudioTurnScope::whole_design(&design);
    store.save_scope(agent, &scope).unwrap();
    let tx = edit(&design, &scope);
    scope.active = false;
    store.save_scope(agent, &scope).unwrap();
    assert!(store.apply_for_agent(agent, &tx).is_err());
    assert_eq!(store.load(design.manifest.id).unwrap(), design);
}

#[test]
fn overview_chat_can_complete_without_edits_or_a_selected_screen() {
    let (_directory, store, design) = fixture();
    let agent = Uuid::new_v4();
    let scope = scope_for_request(&design, None, None);
    store.save_scope(agent, &scope).unwrap();
    let context = store.request_context(agent).unwrap();
    assert!(context["scope"]["current_screen_id"].is_null());
    assert!(context["scope"]["allow_create"].as_bool().unwrap());
    store.verify_turn_review(agent).unwrap();
    assert_eq!(store.load(design.manifest.id).unwrap(), design);
}
#[test]
fn design_agent_can_edit_multiple_screens_while_one_is_selected() {
    let (_directory, store, design) = fixture();
    let agent = Uuid::new_v4();
    let current = design.manifest.screens[0].id;
    let other = Uuid::new_v4();
    let scope = scope_for_request(&design, Some(current), Some("heading".into()));
    store.save_scope(agent, &scope).unwrap();
    let mut transaction = edit(&design, &scope);
    transaction.operations = vec![StudioOperation::CreateScreen {
        screen: StudioScreen {
            id: other,
            name: "Settings".into(),
            ..design.manifest.screens[0].clone()
        },
        document: starter_document(),
    }];
    let design = store.apply_for_agent(agent, &transaction).unwrap();
    let scope = scope_for_request(&design, Some(current), Some("heading".into()));
    store.save_scope(agent, &scope).unwrap();
    let mut transaction = edit(&design, &scope);
    transaction.operations.push(StudioOperation::WriteScreen {
        screen_id: other,
        document: StudioDocument {
            html: "<h1>Settings updated</h1>".into(),
            ..Default::default()
        },
    });
    let changed = store.apply_for_agent(agent, &transaction).unwrap();
    assert_eq!(changed.documents[&current].html, "<h1>Changed</h1>");
    assert_eq!(changed.documents[&other].html, "<h1>Settings updated</h1>");
    assert_eq!(
        store.request_context(agent).unwrap()["scope"]["current_screen_id"],
        current.to_string()
    );
    assert!(!scope.allow_shared_system);
    let mut outside = edit(&changed, &scope);
    outside.design_id = Uuid::new_v4();
    assert!(store.apply_for_agent(agent, &outside).is_err());
    outside = edit(&changed, &scope);
    outside.operations = vec![StudioOperation::SetSystem {
        system: changed.system.clone(),
        expected_system_revision: changed.system.revision,
    }];
    assert!(store.apply_for_agent(agent, &outside).is_err());
}
#[test]
fn a_requested_new_screen_can_be_refined_in_the_same_turn() {
    let (_directory, store, design) = fixture();
    let agent = Uuid::new_v4();
    let scope = scope_for_request(&design, None, None);
    store.save_scope(agent, &scope).unwrap();
    let screen = StudioScreen {
        id: Uuid::new_v4(),
        name: "Settings".into(),
        width: 800,
        height: 600,
        archived: false,
        files: StudioScreenFiles::default(),
    };
    let mut tx = edit(&design, &scope);
    tx.operations = vec![StudioOperation::CreateScreen {
        screen: screen.clone(),
        document: starter_document(),
    }];
    let next = store.apply_for_agent(agent, &tx).unwrap();
    let mut tx = edit(&next, &scope);
    tx.operations = vec![StudioOperation::WriteScreen {
        screen_id: screen.id,
        document: StudioDocument {
            html: "<h1>Settings</h1>".into(),
            ..Default::default()
        },
    }];
    let next = store.apply_for_agent(agent, &tx).unwrap();
    assert_eq!(next.documents[&screen.id].html, "<h1>Settings</h1>");
    store.apply_for_agent(agent, &edit(&next, &scope)).unwrap();
    let undone = store.undo_latest(design.manifest.id).unwrap();
    assert_eq!(
        undone.documents[&design.manifest.screens[0].id],
        design.documents[&design.manifest.screens[0].id]
    );
    assert!(
        undone
            .manifest
            .screens
            .iter()
            .find(|s| s.id == screen.id)
            .unwrap()
            .archived
    );
}
#[test]
fn undo_and_redo_follow_history_and_new_edits_discard_redo() {
    let (_directory, store, design) = fixture();
    let id = design.manifest.id;
    let screen = design.manifest.screens[0].id;
    let change = |design: &StudioDesign, html: &str| {
        let scope = StudioTurnScope::whole_design(design);
        let mut tx = edit(design, &scope);
        tx.operations = vec![StudioOperation::WriteScreen {
            screen_id: screen,
            document: StudioDocument {
                html: html.into(),
                ..Default::default()
            },
        }];
        store.apply(&scope, &tx).unwrap()
    };
    let a = change(&design, "A");
    change(&a, "B");
    assert_eq!(store.undo_latest(id).unwrap().documents[&screen].html, "A");
    assert_eq!(
        store.undo_latest(id).unwrap().documents[&screen],
        design.documents[&screen]
    );
    assert_eq!(store.redo_latest(id).unwrap().documents[&screen].html, "A");
    let current = store.load(id).unwrap();
    change(&current, "C");
    assert!(store.redo_latest(id).is_err());
    assert_eq!(store.load(id).unwrap().documents[&screen].html, "C");
}
#[test]
fn over_limit_asset_transaction_is_rejected_before_journaling() {
    let (_directory, store, design) = fixture();
    let root = store
        .project
        .join(format!("{DESIGNS_DIR}/{}/assets", design.manifest.id));
    fs::create_dir_all(&root).unwrap();
    for index in 0..1000 {
        fs::write(root.join(format!("{index}.png")), b"asset").unwrap();
    }
    let design = store.load(design.manifest.id).unwrap();
    let scope = StudioTurnScope::whole_design(&design);
    let mut tx = edit(&design, &scope);
    tx.operations = vec![StudioOperation::AddAsset {
        name: "overflow.png".into(),
        bytes: vec![1],
    }];
    assert!(store.apply(&scope, &tx).is_err());
    assert!(!root.join("overflow.png").exists());
    assert!(!store
        .cache
        .join("transactions")
        .join(format!("{}.json", tx.id))
        .exists());
    assert_eq!(store.load(design.manifest.id).unwrap(), design);
}
#[test]
fn interrupted_multi_file_transaction_recovers_on_reopen() {
    let (_directory, store, design) = fixture();
    let scope = StudioTurnScope::whole_design(&design);
    let tx = edit(&design, &scope);
    let after = store.apply(&scope, &tx).unwrap();
    let path = store
        .cache
        .join("transactions")
        .join(format!("{}.json", tx.id));
    let mut journal: Journal = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    journal.committed = false;
    // Simulate an interrupted commit: one old file remains, other files are new.
    let html = journal
        .previous
        .iter()
        .find(|(p, _)| p.ends_with("index.html"))
        .unwrap();
    fs::write(store.project.join(html.0), html.1.as_ref().unwrap()).unwrap();
    atomic(&path, &serde_json::to_vec(&journal).unwrap()).unwrap();
    assert_eq!(store.load(design.manifest.id).unwrap(), after);
}

#[test]
fn screen_metadata_and_all_style_writers_register_design_local_overrides() {
    let (_dir, store, design) = fixture();
    let screen = design.manifest.screens[0].id;
    let raw: serde_json::Value = serde_json::from_slice(
        &fs::read(
            store
                .project
                .join(format!("{DESIGNS_DIR}/{}/design.json", design.manifest.id)),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(raw["screens"][0]["files"]["html"], "index.html");
    let mut legacy = raw.clone();
    legacy["screens"][0]
        .as_object_mut()
        .unwrap()
        .remove("files");
    let manifest: StudioDesignManifest = serde_json::from_value(legacy).unwrap();
    assert_eq!(manifest.screens[0].files, StudioScreenFiles::default());
    let agent = Uuid::new_v4();
    let scope = StudioTurnScope::screen(design.manifest.id, screen);
    store.save_scope(agent, &scope).unwrap();
    let mut tx = edit(&design, &scope);
    tx.operations = vec![StudioOperation::WriteScreen {
        screen_id: screen,
        document: StudioDocument {
            html: "<style>h1{color:red}</style><h1 style='padding:17px'>Local</h1>".into(),
            css: "h1{font-size:37px}".into(),
            js: String::new(),
        },
    }];
    let changed = store.apply_for_agent(agent, &tx).unwrap();
    assert_eq!(changed.system, design.system);
    let style = &changed.overrides.screen_styles[&screen];
    assert!(style.stylesheet.ends_with("/styles.css"));
    assert!(style.inline_styles.ends_with("/index.html"));
    let persisted: StudioOverrides = serde_json::from_slice(
        &fs::read(store.project.join(format!(
            "{DESIGNS_DIR}/{}/overrides.json",
            design.manifest.id
        )))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(persisted, changed.overrides);
    let mut malicious = changed.manifest.screens[0].clone();
    malicious.files.html = "../../../AGENTS.md".into();
    let mut tx = edit(&changed, &scope);
    tx.operations = vec![StudioOperation::UpdateScreen { screen: malicious }];
    assert!(store.apply_for_agent(agent, &tx).is_err());
}
#[test]
fn request_context_is_frozen_and_contains_scope_tokens_overrides_and_conventions() {
    let (_dir, store, design) = fixture();
    fs::write(
        store.project.join("AGENTS.md"),
        "Use the existing project buttons.",
    )
    .unwrap();
    let agent = Uuid::new_v4();
    let mut scope = scope_for_request(
        &design,
        Some(design.manifest.screens[0].id),
        Some("title".into()),
    );
    store.save_scope(agent, &scope).unwrap();
    let frozen = store.request_context(agent).unwrap();
    assert_eq!(frozen["scope"]["selected_element"], "title");
    assert_eq!(frozen["fingerprint"], design.fingerprint);
    assert_eq!(
        frozen["effective_tokens"],
        serde_json::to_value(design.tokens()).unwrap()
    );
    assert!(frozen["overrides"]["screen_styles"].is_object());
    assert_eq!(
        frozen["repository_conventions"]["AGENTS.md"],
        "Use the existing project buttons."
    );
    store
        .apply_for_agent(agent, &edit(&design, &scope))
        .unwrap();
    fs::write(store.project.join("AGENTS.md"), "New conventions").unwrap();
    scope.current_screen_id = None;
    store.save_scope(agent, &scope).unwrap();
    assert_eq!(store.request_context(agent).unwrap(), frozen);
}
#[test]
fn completion_requires_current_screenshot_and_passed_review() {
    let (_dir, store, design) = fixture();
    let agent = Uuid::new_v4();
    let screen = design.manifest.screens[0].id;
    let scope = StudioTurnScope::screen(design.manifest.id, screen);
    store.save_scope(agent, &scope).unwrap();
    assert!(store.verify_turn_review(agent).is_ok());
    let changed = store
        .apply_for_agent(agent, &edit(&design, &scope))
        .unwrap();
    assert!(store.verify_turn_review(agent).is_err());
    assert!(store
        .review_screen(
            agent,
            screen,
            &changed.fingerprint,
            "Readable heading and balanced spacing"
        )
        .is_err());
    atomic(&store.thumbnail_path(&changed, screen), b"fixture image").unwrap();
    store.record_snapshot_view(agent, &changed, screen).unwrap();
    store
        .review_screen(
            agent,
            screen,
            &changed.fingerprint,
            "Readable heading and balanced spacing",
        )
        .unwrap();
    store.verify_turn_review(agent).unwrap();
    let mut tx = edit(&changed, &scope);
    if let StudioOperation::WriteScreen { document, .. } = &mut tx.operations[0] {
        document.html.push_str("<p>New copy</p>");
    }
    let latest = store.apply_for_agent(agent, &tx).unwrap();
    assert!(store.verify_turn_review(agent).is_err());
    assert!(store
        .review_screen(agent, screen, &latest.fingerprint, "Old screenshot")
        .is_err());
    assert!(store
        .review_screen(agent, screen, &changed.fingerprint, "Stale fingerprint")
        .is_err());
}
#[test]
fn saved_revisions_keep_original_source_assets_and_queue_requested_screens() {
    let (_dir, store, design) = fixture();
    let id = design.manifest.id;
    let screen = design.manifest.screens[0].id;
    let path = store
        .project
        .join(format!("{DESIGNS_DIR}/{id}/assets/sample.png"));
    atomic(&path, b"original image").unwrap();
    let old = store.load(id).unwrap();
    let scope = StudioTurnScope::screen(id, screen);
    let next = store.apply(&scope, &edit(&old, &scope)).unwrap();
    atomic(&path, b"replacement image").unwrap();
    let saved = store
        .saved_revision(id, old.manifest.revision, &old.fingerprint)
        .unwrap();
    assert_eq!(saved.design.documents, old.documents);
    assert_eq!(saved.assets["assets/sample.png"], b"original image");
    store.request_thumbnail(&saved.design, screen).unwrap();
    assert!(store.has_requested_thumbnails().unwrap());
    let jobs = store.requested_thumbnails().unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].fingerprint, old.fingerprint);
    assert_ne!(
        store.thumbnail_path(&saved.design, screen),
        store.thumbnail_path(&next, screen)
    );
    atomic(&store.thumbnail_path(&saved.design, screen), b"rendered").unwrap();
    assert!(store.requested_thumbnails().unwrap().is_empty());
    assert!(!store.has_requested_thumbnails().unwrap());
    assert!(store
        .saved_revision(Uuid::new_v4(), old.manifest.revision, &old.fingerprint)
        .is_err());
    assert!(store.saved_revision(id, 999, &old.fingerprint).is_err());
}
#[test]
fn interleaved_manual_save_does_not_split_agent_undo_or_lose_manual_css() {
    let (_dir, store, design) = fixture();
    let screen = design.manifest.screens[0].id;
    let agent_scope = StudioTurnScope::screen(design.manifest.id, screen);
    let mut first_tx = edit(&design, &agent_scope);
    if let StudioOperation::WriteScreen { document, .. } = &mut first_tx.operations[0] {
        document.css = design.documents[&screen].css.clone();
        document.js = design.documents[&screen].js.clone();
    }
    let first = store.apply(&agent_scope, &first_tx).unwrap();
    let manual_scope = StudioTurnScope::screen(design.manifest.id, screen);
    let mut manual = edit(&first, &manual_scope);
    let mut doc = first.documents[&screen].clone();
    doc.css = "h1{padding:21px}".into();
    manual.operations = vec![StudioOperation::WriteScreen {
        screen_id: screen,
        document: doc,
    }];
    let manual = store.apply(&manual_scope, &manual).unwrap();
    let mut last = edit(&manual, &agent_scope);
    let mut doc = manual.documents[&screen].clone();
    doc.html = "<h1>Agent finished</h1>".into();
    last.operations = vec![StudioOperation::WriteScreen {
        screen_id: screen,
        document: doc,
    }];
    let completed = store.apply(&agent_scope, &last).unwrap();
    let undo = store.undo_latest(design.manifest.id).unwrap();
    assert_eq!(undo.documents[&screen].html, design.documents[&screen].html);
    assert_eq!(undo.documents[&screen].css, manual.documents[&screen].css);
    let redo = store.redo_latest(design.manifest.id).unwrap();
    assert_eq!(redo.documents, completed.documents);
    store.undo_latest(design.manifest.id).unwrap();
    let original = store.undo_latest(design.manifest.id).unwrap();
    assert_eq!(original.documents, design.documents);
}
#[test]
fn source_association_is_scoped_and_immutable_in_handoff() {
    let (_dir, store, design) = fixture();
    let screen = design.manifest.screens[0].id;
    let narrow = StudioTurnScope::screen(design.manifest.id, screen);
    let task = crate::TaskRef {
        provider: crate::IssueTrackerProvider::Personal, site_url: "local".into(),
        issue_id: "task-12".into(), issue_key: "TASK-12".into(), issue_url: String::new(), title: "Build this screen".into(),
    };
    let op = StudioOperation::SetSource {
        document: Some(StudioSource {
            task_ref: None,
            reference: "docs/spec.md".into(),
            content: "Original document".into(),
        }),
        task: Some(StudioSource {
            task_ref: Some(task.clone()),
            reference: "TASK-12".into(),
            content: "Build this screen".into(),
        }),
    };
    let mut tx = edit(&design, &narrow);
    tx.operations = vec![op.clone()];
    assert!(store.apply(&narrow, &tx).is_err());
    let whole = StudioTurnScope::whole_design(&design);
    tx.scope_id = whole.id;
    let changed = store.apply(&whole, &tx).unwrap();
    let reopened = store.load(design.manifest.id).unwrap();
    assert!(reopened.manifest.links_doc(std::path::Path::new("docs/spec.md")));
    assert!(reopened.manifest.links_task(&task));
    let mut renamed_task = task.clone();
    renamed_task.title = "A renamed task".into();
    renamed_task.issue_url = "https://tracker.example/new-link".into();
    assert!(reopened.manifest.links_task(&renamed_task));
    renamed_task.issue_key = "TASK-13".into();
    assert!(!reopened.manifest.links_task(&renamed_task));
    let snapshot = store.handoff(design.manifest.id, None).unwrap();
    assert_eq!(snapshot.design.manifest.linked_task(), Some(&task));
    let agent = Uuid::new_v4();
    store.link_implementation_agent(snapshot.id, agent).unwrap();
    store.link_implementation_agent(snapshot.id, agent).unwrap();
    assert_eq!(store.implementation_agents(design.manifest.id).unwrap(), vec![agent]);
    assert_eq!(
        snapshot.design.manifest.source_context["document"].content,
        "Original document"
    );
    let mut tx = edit(&changed, &whole);
    tx.operations = vec![StudioOperation::SetSource {
        document: None,
        task: None,
    }];
    store.apply(&whole, &tx).unwrap();
    assert!(!store.load(design.manifest.id).unwrap().manifest.links_task(&task));
    assert!(store.read_handoff(snapshot.id).unwrap().design.manifest.links_task(&task));
    assert_eq!(
        store
            .read_handoff(snapshot.id)
            .unwrap()
            .design
            .manifest
            .source_task
            .as_deref(),
        Some("TASK-12")
    );
}

#[test]
fn overlapping_manual_and_agent_history_reports_conflict_without_writing() {
    let (_dir, store, design) = fixture();
    let screen = design.manifest.screens[0].id;
    let scope = StudioTurnScope::screen(design.manifest.id, screen);
    let first = store.apply(&scope, &edit(&design, &scope)).unwrap();
    let manual = StudioTurnScope::screen(design.manifest.id, screen);
    let mut tx = edit(&first, &manual);
    if let StudioOperation::WriteScreen { document, .. } = &mut tx.operations[0] {
        document.css = "h1{color:blue}".into();
    }
    let middle = store.apply(&manual, &tx).unwrap();
    let mut tx = edit(&middle, &scope);
    if let StudioOperation::WriteScreen { document, .. } = &mut tx.operations[0] {
        document.html = "<h1>Done</h1>".into();
        document.css = middle.documents[&screen].css.clone();
    }
    let last = store.apply(&scope, &tx).unwrap();
    assert!(store
        .undo_latest(design.manifest.id)
        .unwrap_err()
        .to_string()
        .contains("conflict"));
    assert_eq!(store.load(design.manifest.id).unwrap(), last);
}
#[test]
fn duplicate_provider_completion_does_not_fail_an_already_reviewed_turn() {
    let (_dir, store, design) = fixture();
    let agent = Uuid::new_v4();
    let mut scope = StudioTurnScope::whole_design(&design);
    store.save_scope(agent, &scope).unwrap();
    store.verify_turn_review(agent).unwrap();
    scope.active = false;
    store.save_scope(agent, &scope).unwrap();
    store.verify_turn_review(agent).unwrap();
    let other = Uuid::new_v4();
    let mut scope = StudioTurnScope::whole_design(&design);
    store.save_scope(other, &scope).unwrap();
    scope.active = false;
    store.save_scope(other, &scope).unwrap();
    assert!(store.verify_turn_review(other).is_err());
}

#[test]
fn captured_sources_cannot_produce_an_unreadable_manifest() {
    let (_dir, store, design) = fixture();
    let scope = StudioTurnScope::whole_design(&design);
    let mut tx = edit(&design, &scope);
    tx.operations = vec![StudioOperation::SetSource {
        document: Some(StudioSource {
            task_ref: None,
            reference: "large.md".into(),
            content: "\"".repeat(2 * 1024 * 1024),
        }),
        task: None,
    }];
    assert!(store
        .apply(&scope, &tx)
        .unwrap_err()
        .to_string()
        .contains("file limit"));
    assert_eq!(store.load(design.manifest.id).unwrap(), design);
    assert!(!store
        .cache
        .join("transactions")
        .join(format!("{}.json", tx.id))
        .exists());
}

fn system_edit(store: &StudioStore, id: Uuid, system: StudioDesignSystem) -> StudioDesign {
    let design = store.load(id).unwrap();
    let scope = scope_for_request(&design, Some(id), None);
    let mut tx = edit(&design, &scope);
    tx.operations = vec![StudioOperation::SetSystem {
        system,
        expected_system_revision: design.system.revision,
    }];
    store.apply(&scope, &tx).unwrap()
}
fn bind_system(store: &StudioStore, design: &StudioDesign, id: Option<Uuid>) -> StudioDesign {
    let mut scope = StudioTurnScope::whole_design(design);
    scope.allow_system_binding = true;
    let mut tx = edit(design, &scope);
    tx.operations = vec![StudioOperation::BindSystem {
        system_id: id,
        expected_system_fingerprint: id.map(|id| store.system_binding_fingerprint(id).unwrap()),
    }];
    store.apply(&scope, &tx).unwrap()
}
#[test]
fn named_systems_are_independent_and_handoffs_freeze_applied_values() {
    let (_dir, store, design) = fixture();
    assert!(design.system.tokens.is_empty());
    assert!(!store
        .project
        .join("choro_designs/design-system/system.json")
        .exists());
    let web = store.create_system("Web", "Web", None).unwrap();
    let draft = system_edit(&store, web.id, StudioDesignSystem::default());
    store
        .publish_system(
            web.id,
            draft.manifest.revision,
            &store.load(web.id).unwrap().fingerprint,
            &[],
        )
        .unwrap();
    let mobile = store.create_system("Mobile", "iOS", Some(web.id)).unwrap();
    let mut mobile_tokens = mobile.draft.clone();
    mobile_tokens
        .tokens
        .insert("color-primary".into(), "#abcdef".into());
    let mobile_draft = system_edit(&store, mobile.id, mobile_tokens);
    store
        .publish_system(
            mobile.id,
            mobile_draft.manifest.revision,
            &store.load(mobile.id).unwrap().fingerprint,
            &[],
        )
        .unwrap();
    let design = bind_system(&store, &design, Some(web.id));
    let other = bind_system(
        &store,
        &store.create("Mobile screens").unwrap(),
        Some(mobile.id),
    );
    let handoff = store.handoff(design.manifest.id, None).unwrap();
    let mut next = draft.system;
    next.tokens.insert("color-primary".into(), "#123456".into());
    let updated = system_edit(&store, web.id, next);
    store
        .publish_system(
            web.id,
            updated.manifest.revision,
            &store.load(web.id).unwrap().fingerprint,
            &store.system_impact(web.id).unwrap(),
        )
        .unwrap();
    assert_eq!(store.load(other.manifest.id).unwrap(), other);
    assert_eq!(
        store.read_handoff(handoff.id).unwrap().design.tokens()["color-primary"],
        "#335cff"
    );
    assert_eq!(
        store.load(design.manifest.id).unwrap().tokens()["color-primary"],
        "#123456"
    );
    store.set_default_system(Some(mobile.id)).unwrap();
    assert_eq!(
        store.create("Next mobile").unwrap().system,
        mobile_draft.system
    );
}
#[test]
fn system_agent_cannot_write_screens_and_draft_history_survives_reopen() {
    let (_dir, store, _) = fixture();
    let record = store.create_system("Admin", "Web", None).unwrap();
    let draft = store.load(record.id).unwrap();
    let scope = scope_for_request(&draft, Some(record.id), None);
    assert!(scope.allow_shared_system);
    assert!(!scope.allow_create);
    assert!(store.apply(&scope, &edit(&draft, &scope)).is_err());
    let changed = system_edit(&store, record.id, StudioDesignSystem::default());
    let undone = store.undo_latest(record.id).unwrap();
    assert!(undone.system.tokens.is_empty());
    let redone = store.redo_latest(record.id).unwrap();
    assert_eq!(redone.system.tokens, changed.system.tokens);
    let reopened = StudioStore {
        project: store.project.clone(),
        cache: store.cache.clone(),
    };
    assert_eq!(reopened.load(record.id).unwrap(), redone);
    let context = context_from_path(&PathBuf::from(format!(
        ".choro/assistants/studio-systems/{}/{}",
        record.id,
        Uuid::new_v4()
    )))
    .unwrap();
    assert_eq!(context.target, StudioAgentTarget::DesignSystem);
    assert!(system_prompt(&context).contains("DRAFT"));
    store
        .archive_system(record.id, redone.manifest.revision, true)
        .unwrap();
    let scope = scope_for_request(&redone, Some(record.id), None);
    assert!(store.apply(&scope, &edit(&redone, &scope)).is_err());
    store
        .archive_system(record.id, redone.manifest.revision, false)
        .unwrap();
}
#[test]
fn publishing_revalidates_consumers_and_missing_tokens_block_switching() {
    let (_dir, store, design) = fixture();
    let record = store.create_system("Web", "Web", None).unwrap();
    let draft = system_edit(&store, record.id, StudioDesignSystem::default());
    store
        .publish_system(
            record.id,
            draft.manifest.revision,
            &store.load(record.id).unwrap().fingerprint,
            &[],
        )
        .unwrap();
    let design = bind_system(&store, &design, Some(record.id));
    let impacts = store.system_impact(record.id).unwrap();
    let mut scope = StudioTurnScope::whole_design(&design);
    scope.allow_system_binding = true;
    store.apply(&scope, &edit(&design, &scope)).unwrap();
    assert!(store
        .publish_system(
            record.id,
            draft.manifest.revision,
            &store.load(record.id).unwrap().fingerprint,
            &impacts
        )
        .is_err());
    let design = store.load(design.manifest.id).unwrap();
    let mut tx = edit(&design, &scope);
    tx.operations = vec![StudioOperation::WriteScreen {
        screen_id: design.manifest.screens[0].id,
        document: StudioDocument {
            html: "<h1>Hello</h1>".into(),
            css: "h1{color:var(--color-primary)}".into(),
            js: String::new(),
        },
    }];
    let design = store.apply(&scope, &tx).unwrap();
    let mut tx = edit(&design, &scope);
    tx.operations = vec![StudioOperation::BindSystem {
        system_id: None,
        expected_system_fingerprint: None,
    }];
    assert!(store
        .apply(&scope, &tx)
        .unwrap_err()
        .to_string()
        .contains("missing tokens"));
    assert_eq!(store.load(design.manifest.id).unwrap(), design);
}
#[test]
fn legacy_system_import_preserves_source_files_and_effective_styles() {
    let (_dir, store, design) = fixture();
    let path = store
        .project
        .join("choro_designs/design-system/system.json");
    let system = StudioDesignSystem::default();
    atomic(&path, &serde_json::to_vec(&system).unwrap()).unwrap();
    let manifest_path = store
        .project
        .join(format!("choro_designs/{}/design.json", design.manifest.id));
    let mut manifest = design.manifest;
    manifest.design_system = "../design-system/system.json".into();
    atomic(&manifest_path, &serde_json::to_vec(&manifest).unwrap()).unwrap();
    let before = store.load(manifest.id).unwrap();
    let original = fs::read(&path).unwrap();
    let records = store.systems().unwrap();
    assert_eq!(records[0].name, "Legacy starter");
    assert_eq!(store.load(manifest.id).unwrap().tokens(), before.tokens());
    assert_eq!(fs::read(path).unwrap(), original);
    assert_eq!(store.systems().unwrap().len(), 1);
}

#[test]
fn draft_assets_stay_private_until_publication_and_switch_requires_host_scope() {
    let (_dir, store, design) = fixture();
    let record = store.create_system("Web", "Web", None).unwrap();
    let draft = system_edit(&store, record.id, StudioDesignSystem::default());
    store
        .publish_system(
            record.id,
            draft.manifest.revision,
            &store.load(record.id).unwrap().fingerprint,
            &[],
        )
        .unwrap();
    let design = bind_system(&store, &design, Some(record.id));
    let scope = scope_for_request(&design, Some(design.manifest.screens[0].id), None);
    let mut tx = edit(&design, &scope);
    tx.operations = vec![StudioOperation::BindSystem {
        system_id: None,
        expected_system_fingerprint: None,
    }];
    assert!(store.apply(&scope, &tx).is_err());
    let scope = scope_for_request(&draft, Some(record.id), None);
    let mut tx = edit(&draft, &scope);
    tx.operations = vec![StudioOperation::AddAsset {
        name: "sample.png".into(),
        bytes: vec![1, 2, 3],
    }];
    let changed = store.apply(&scope, &tx).unwrap();
    assert_eq!(store.load(design.manifest.id).unwrap(), design);
    assert!(!store
        .assets(design.manifest.id)
        .unwrap()
        .contains_key("design-system/assets/sample.png"));
    store
        .publish_system(
            record.id,
            changed.manifest.revision,
            &store.load(record.id).unwrap().fingerprint,
            &store.system_impact(record.id).unwrap(),
        )
        .unwrap();
    assert_eq!(
        store.assets(design.manifest.id).unwrap()["design-system/assets/sample.png"],
        vec![1, 2, 3]
    );
    let copied = store
        .create_system("Copy", "Android", Some(record.id))
        .unwrap();
    assert_eq!(copied.platform, "Android");
    assert_eq!(
        store.assets(copied.id).unwrap()["design-system/assets/sample.png"],
        vec![1, 2, 3]
    );
}

#[test]
fn comparison_previews_do_not_write_designs_and_system_context_is_frozen() {
    let (_dir, store, design) = fixture();
    let record = store.create_system("Mobile", "iOS", None).unwrap();
    let draft = system_edit(&store, record.id, StudioDesignSystem::default());
    let scope = scope_for_request(&draft, Some(record.id), None);
    let agent = Uuid::new_v4();
    store.save_scope(agent, &scope).unwrap();
    let frozen = store.request_context(agent).unwrap();
    assert_eq!(frozen["design_system_context"]["platform"], "iOS");
    assert_eq!(frozen["design_system_context"]["draft_workspace"], true);
    let comparisons = store
        .system_comparison(record.id, Some(record.id), true)
        .unwrap();
    assert_eq!(comparisons.len(), 1);
    let (_, before, after) = &comparisons[0];
    assert!(before.system.tokens.is_empty());
    assert_eq!(after.system, draft.system);
    assert!(!store
        .project
        .join(format!("choro_designs/{}", before.manifest.id))
        .exists());
    assert!(store
        .saved_revision(
            after.manifest.id,
            after.manifest.revision,
            &after.fingerprint
        )
        .is_ok());
    assert_eq!(store.load(design.manifest.id).unwrap(), design);
    assert_eq!(store.request_context(agent).unwrap(), frozen);
    if let Ok(path) = std::env::var("CHORO_SYSTEM_SPECIMEN_FIXTURE") {
        let screen = draft.manifest.screens[0].clone();
        let fixture = serde_json::json!({"system_specimen":true,"session":"test","screen_id":screen.id,"document":draft.documents[&screen.id],"revision":draft.manifest.revision,"fingerprint":draft.fingerprint,"tokens":draft.tokens(),"tokens_css":draft.tokens_css(),"screens":draft.manifest.screens,"width":screen.width,"height":screen.height,"assets":{},"thumbnail":false,"mode":"preview"});
        fs::write(path, serde_json::to_vec_pretty(&fixture).unwrap()).unwrap();
    }
}

#[test]
fn font_files_and_missing_references_are_validated_before_publish() {
    let (_dir, store, _) = fixture();
    let record = store.create_system("Web", "Web", None).unwrap();
    let mut system = StudioDesignSystem::default();
    system.font_faces.push(StudioFontFace {
        family: "Product Sans".into(),
        file: "product.woff2".into(),
        weight: 400,
        italic: false,
    });
    let draft = system_edit(&store, record.id, system.clone());
    assert!(store
        .publish_system(
            record.id,
            draft.manifest.revision,
            &store.load(record.id).unwrap().fingerprint,
            &[]
        )
        .is_err());
    assert!(draft
        .tokens_css()
        .contains("design-system/assets/product.woff2"));
    system.font_faces[0].file = "../escape.woff2".into();
    let scope = scope_for_request(&draft, Some(record.id), None);
    let mut tx = edit(&draft, &scope);
    tx.operations = vec![StudioOperation::SetSystem {
        system,
        expected_system_revision: draft.system.revision,
    }];
    assert!(store.apply(&scope, &tx).is_err());
}

#[test]
fn reviewed_publication_rejects_external_edits_without_revision_bump() {
    let (_dir, store, _) = fixture();
    let record = store.create_system("Web", "Web", None).unwrap();
    let draft = system_edit(&store, record.id, StudioDesignSystem::default());
    let path = store.project.join(format!(
        "choro_designs/design-systems/{}/system.json",
        record.id
    ));
    let mut external = store.system(record.id).unwrap();
    external
        .draft
        .tokens
        .insert("color-primary".into(), "#abcdef".into());
    fs::write(&path, serde_json::to_vec(&external).unwrap()).unwrap();
    assert!(store
        .publish_system(record.id, draft.manifest.revision, &draft.fingerprint, &[])
        .is_err());
    assert!(store.system(record.id).unwrap().applied.is_none());
}
