use super::*;
use std::fs;

fn fixture() -> (tempfile::TempDir, StudioStore, StudioDesign) {
    let directory = tempfile::tempdir().unwrap();
    let project = directory.path().join("project");
    fs::create_dir(&project).unwrap();
    let store = StudioStore::new(project, directory.path().join("data")).unwrap();
    let design = store.create("Sections").unwrap();
    (directory, store, design)
}
fn tx(design: &StudioDesign, scope: &StudioTurnScope, operations: Vec<StudioOperation>) -> StudioTransaction {
    StudioTransaction {
        id: Uuid::new_v4(),
        scope_id: scope.id,
        design_id: design.manifest.id,
        expected_revision: design.manifest.revision,
        expected_fingerprint: design.fingerprint.clone(),
        operations,
    }
}
fn apply(store: &StudioStore, design: &StudioDesign, operations: Vec<StudioOperation>) -> anyhow::Result<StudioDesign> {
    let scope = StudioTurnScope::whole_design(design);
    store.apply(&scope, &tx(design, &scope, operations))
}
fn screen(name: &str, width: u32, height: u32) -> StudioScreen {
    StudioScreen {
        id: Uuid::new_v4(),
        name: name.into(),
        width,
        height,
        archived: false,
        files: StudioScreenFiles::default(),
    }
}
fn create(screen: &StudioScreen, section_id: Option<Option<Uuid>>) -> StudioOperation {
    StudioOperation::CreateScreen {
        screen: screen.clone(),
        document: starter_document(),
        section_id,
    }
}
/// Home plus phone, tablet and desktop screens.
fn with_screens(store: &StudioStore, design: &StudioDesign) -> (StudioDesign, Vec<StudioScreen>) {
    let screens = vec![
        screen("Feed", 390, 844),
        screen("Composer", 768, 1024),
        screen("Gallery", 1440, 960),
    ];
    let ops = screens.iter().map(|s| create(s, Some(None))).collect();
    (apply(store, design, ops).unwrap(), screens)
}
fn ids(section: &StudioSection) -> Vec<Uuid> {
    section.screen_ids.clone()
}

#[test]
fn old_designs_keep_exact_manifest_bytes_and_load_without_sections() {
    let (_directory, store, design) = fixture();
    let path = store
        .project
        .join(format!("{DESIGNS_DIR}/{}/design.json", design.manifest.id));
    let bytes = fs::read_to_string(&path).unwrap();
    assert!(!bytes.contains("sections") && !bytes.contains("section_layout"));
    let loaded = store.load(design.manifest.id).unwrap();
    assert_eq!(loaded.fingerprint, design.fingerprint);
    assert!(loaded.manifest.sections.is_empty());
    assert_eq!(loaded.manifest.schema_version, 1);
    // A legacy manifest written before sections existed still deserializes.
    let mut legacy: serde_json::Value = serde_json::from_str(&bytes).unwrap();
    legacy.as_object_mut().unwrap().remove("sections");
    let parsed: StudioDesignManifest = serde_json::from_value(legacy).unwrap();
    assert_eq!(parsed.section_layout, StudioSectionLayout::default());
}

#[test]
fn flows_are_created_moved_reordered_and_ungrouped_without_losing_content() {
    let (_directory, store, design) = fixture();
    let (design, screens) = with_screens(&store, &design);
    let home = design.manifest.screens[0].id;
    let (add_post, add_images, mobile) = (
        StudioSection::new("Add post"),
        StudioSection::new("Add images"),
        StudioSection::new("Mobile app"),
    );
    let design = apply(
        &store,
        &design,
        vec![
            StudioOperation::CreateSection { section: add_post.clone() },
            StudioOperation::CreateSection { section: add_images.clone() },
            StudioOperation::CreateSection {
                section: StudioSection {
                    screen_ids: vec![screens[0].id],
                    ..mobile.clone()
                },
            },
        ],
    )
    .unwrap();
    assert_eq!(design.manifest.sections.len(), 3);
    // Each move is one saved edit.
    let design = apply(&store, &design, vec![StudioOperation::MoveScreenToSection {
        screen_id: screens[1].id,
        section_id: Some(add_post.id),
        before_screen_id: None,
    }])
    .unwrap();
    let design = apply(&store, &design, vec![StudioOperation::MoveScreenToSection {
        screen_id: home,
        section_id: Some(add_post.id),
        before_screen_id: Some(screens[1].id),
    }])
    .unwrap();
    assert_eq!(ids(design.manifest.section(add_post.id).unwrap()), vec![home, screens[1].id]);
    // Moving into another section leaves the previous one: membership is exclusive.
    let design = apply(&store, &design, vec![StudioOperation::MoveScreenToSection {
        screen_id: screens[0].id,
        section_id: Some(add_post.id),
        before_screen_id: None,
    }])
    .unwrap();
    assert!(design.manifest.section(mobile.id).unwrap().screen_ids.is_empty());
    let design = apply(&store, &design, vec![StudioOperation::ReorderSectionScreens {
        section_id: add_post.id,
        screen_ids: vec![screens[0].id, home, screens[1].id],
    }])
    .unwrap();
    let design = apply(&store, &design, vec![StudioOperation::ReorderSections {
        section_ids: vec![mobile.id, add_post.id, add_images.id],
    }])
    .unwrap();
    assert_eq!(
        design.manifest.sections.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
        ["Mobile app", "Add post", "Add images"]
    );
    let documents = design.documents.clone();
    // Ungrouping the last section clears the board origin; others keep it.
    let mut solo = design.manifest.clone();
    solo.section_layout.origin = Some(StudioSectionOrigin { x: 1, y: 2 });
    solo.sections.truncate(1);
    let only = solo.sections[0].id;
    solo.ungroup_section(only).unwrap();
    assert_eq!(solo.section_layout.origin, None);
    let ungrouped = apply(&store, &design, vec![StudioOperation::UngroupSection { section_id: add_post.id }]).unwrap();
    assert_eq!(ungrouped.documents, documents, "Ungroup keeps every screen and its content");
    assert_eq!(ungrouped.manifest.screens.len(), 4);
    assert!(ungrouped.manifest.section_of(home).is_none());
    // Undo restores the grouping; redo removes it again; content never changes.
    let undone = store.undo_latest(design.manifest.id).unwrap();
    assert_eq!(undone.manifest.sections, design.manifest.sections);
    assert_eq!(undone.documents, documents);
    let redone = store.redo_latest(design.manifest.id).unwrap();
    assert_eq!(redone.manifest.sections, ungrouped.manifest.sections);
    assert_eq!(redone.documents, documents);
}

#[test]
fn moving_to_unsectioned_reorders_relative_to_unsectioned_screens() {
    let (_directory, store, design) = fixture();
    let (design, screens) = with_screens(&store, &design);
    let section = StudioSection { screen_ids: vec![screens[2].id], ..StudioSection::new("Flow") };
    let design = apply(&store, &design, vec![StudioOperation::CreateSection { section: section.clone() }]).unwrap();
    let design = apply(&store, &design, vec![StudioOperation::MoveScreenToSection {
        screen_id: screens[2].id,
        section_id: None,
        before_screen_id: Some(screens[0].id),
    }])
    .unwrap();
    let order: Vec<_> = design.manifest.screens.iter().map(|s| s.id).collect();
    assert_eq!(order[1], screens[2].id);
    assert_eq!(order[2], screens[0].id);
    assert!(design.manifest.section(section.id).unwrap().screen_ids.is_empty());
    // before_screen_id inside a section is rejected for an Unsectioned move.
    let design = apply(&store, &design, vec![StudioOperation::MoveScreenToSection {
        screen_id: screens[1].id,
        section_id: Some(section.id),
        before_screen_id: None,
    }])
    .unwrap();
    assert!(apply(&store, &design, vec![StudioOperation::MoveScreenToSection {
        screen_id: screens[0].id,
        section_id: None,
        before_screen_id: Some(screens[1].id),
    }])
    .is_err());
}

#[test]
fn invalid_references_ownership_spacing_and_counts_are_rejected_atomically() {
    let (_directory, store, design) = fixture();
    let (design, screens) = with_screens(&store, &design);
    let section = StudioSection::new("Flow");
    let design = apply(&store, &design, vec![StudioOperation::CreateSection { section: section.clone() }]).unwrap();
    let rejected = [
        vec![StudioOperation::CreateSection { section: StudioSection { screen_ids: vec![Uuid::new_v4()], ..StudioSection::new("Missing") } }],
        vec![StudioOperation::CreateSection { section: section.clone() }],
        vec![StudioOperation::CreateSection { section: StudioSection::new("  ") }],
        vec![StudioOperation::CreateSection { section: StudioSection::new("x".repeat(MAX_SECTION_NAME + 1)) }],
        vec![StudioOperation::CreateSection { section: StudioSection { screen_ids: vec![screens[0].id, screens[0].id], ..StudioSection::new("Twice") } }],
        vec![StudioOperation::UpdateSection { section_id: section.id, name: None, direction: None, gap: Some(MAX_SECTION_GAP + 1), title_style: None, header_alignment: None }],
        vec![StudioOperation::SetSectionLayout { direction: StudioSectionArrangement::SideBySide, origin: Some(StudioSectionOrigin { x: MAX_BOARD_COORDINATE + 1, y: 0 }) }],
        vec![StudioOperation::MoveScreenToSection { screen_id: Uuid::new_v4(), section_id: Some(section.id), before_screen_id: None }],
        vec![StudioOperation::MoveScreenToSection { screen_id: screens[0].id, section_id: Some(Uuid::new_v4()), before_screen_id: None }],
        vec![StudioOperation::ReorderSectionScreens { section_id: section.id, screen_ids: vec![screens[0].id] }],
        vec![StudioOperation::ReorderSections { section_ids: vec![] }],
        vec![StudioOperation::UngroupSection { section_id: Uuid::new_v4() }],
        // Replacing with duplicated ownership fails whole-design validation.
        vec![StudioOperation::ReplaceSections {
            sections: vec![
                StudioSection { screen_ids: vec![screens[0].id], ..StudioSection::new("A") },
                StudioSection { screen_ids: vec![screens[0].id], ..StudioSection::new("B") },
            ],
            section_layout: Default::default(),
        }],
        // A valid first step does not commit when a later one fails.
        vec![
            StudioOperation::RenameDesign { name: "Renamed".into() },
            StudioOperation::UngroupSection { section_id: Uuid::new_v4() },
        ],
    ];
    for operations in rejected {
        assert!(apply(&store, &design, operations.clone()).is_err(), "{operations:?}");
        assert_eq!(store.load(design.manifest.id).unwrap(), design);
    }
    let many = (0..=MAX_SECTIONS)
        .map(|i| StudioOperation::CreateSection { section: StudioSection::new(format!("Flow {i}")) })
        .collect();
    assert!(apply(&store, &design, many).is_err());
    // A manifest edited on disk with duplicated ownership fails to load.
    let path = store
        .project
        .join(format!("{DESIGNS_DIR}/{}/design.json", design.manifest.id));
    let mut manifest = design.manifest.clone();
    manifest.sections = vec![
        StudioSection { screen_ids: vec![screens[1].id], ..StudioSection::new("A") },
        StudioSection { screen_ids: vec![screens[1].id], ..StudioSection::new("B") },
    ];
    fs::write(&path, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
    assert!(store.load(design.manifest.id).is_err());
}

#[test]
fn whole_design_scope_is_required_for_grouping() {
    let (_directory, store, design) = fixture();
    let scope = StudioTurnScope::screen(design.manifest.id, design.manifest.screens[0].id);
    let section = StudioSection::new("Flow");
    assert!(store
        .apply(&scope, &tx(&design, &scope, vec![StudioOperation::CreateSection { section }]))
        .is_err());
}

#[test]
fn stale_grouping_edits_conflict_instead_of_merging_and_retries_are_idempotent() {
    let (_directory, store, design) = fixture();
    let (design, screens) = with_screens(&store, &design);
    let section = StudioSection::new("Flow");
    let scope = StudioTurnScope::whole_design(&design);
    let first = tx(&design, &scope, vec![StudioOperation::CreateSection { section: section.clone() }]);
    let created = store.apply(&scope, &first).unwrap();
    // Retrying the same request returns the same result without a second edit.
    assert_eq!(store.apply(&scope, &first).unwrap(), created);
    let stale = tx(&design, &scope, vec![StudioOperation::CreateSection { section: StudioSection::new("Other") }]);
    let error = store.apply(&scope, &stale).unwrap_err().to_string();
    assert!(error.contains("conflict"), "{error}");
    let stale_move = tx(&design, &scope, vec![StudioOperation::MoveScreenToSection {
        screen_id: screens[0].id,
        section_id: None,
        before_screen_id: None,
    }]);
    assert!(store.apply(&scope, &stale_move).is_err());
    assert_eq!(store.load(design.manifest.id).unwrap(), created);
}

#[test]
fn create_screen_section_is_presence_sensitive() {
    let (_directory, store, design) = fixture();
    let section = StudioSection::new("Add post");
    let design = apply(&store, &design, vec![StudioOperation::CreateSection { section: section.clone() }]).unwrap();
    let agent = Uuid::new_v4();
    let scope = scope_for_section_request(&design, section.id);
    assert_eq!(scope.current_section_id, Some(section.id));
    assert_eq!(scope.current_screen_id, None);
    assert_eq!(scope.selected_element, None);
    store.save_scope(agent, &scope).unwrap();
    let parse = |value: serde_json::Value| serde_json::from_value::<StudioOperation>(value).unwrap();
    let raw = |s: &StudioScreen| serde_json::json!({"operation":"create_screen","screen":s,"document":starter_document()});
    let (omitted, null, explicit) = (screen("Draft", 390, 844), screen("Loose", 390, 844), screen("Pinned", 390, 844));
    let mut with_null = raw(&null);
    with_null["section_id"] = serde_json::Value::Null;
    let mut with_id = raw(&explicit);
    with_id["section_id"] = serde_json::json!(section.id);
    let operations = vec![parse(raw(&omitted)), parse(with_null), parse(with_id)];
    // Presence round-trips, so the idempotency journal compares identical requests.
    let encoded = serde_json::to_value(&operations).unwrap();
    assert!(encoded[0].get("section_id").is_none());
    assert!(encoded[1]["section_id"].is_null() && encoded[1].get("section_id").is_some());
    assert_eq!(encoded[2]["section_id"], serde_json::json!(section.id));
    let applied = store.apply_for_agent(agent, &tx(&design, &scope, operations)).unwrap();
    assert_eq!(ids(applied.manifest.section(section.id).unwrap()), vec![omitted.id, explicit.id]);
    assert!(applied.manifest.section_of(null.id).is_none());
    // The frozen section stays the default even after the user ungroups it,
    // and omission then fails instead of silently landing elsewhere.
    let ungrouped = apply(&store, &applied, vec![StudioOperation::UngroupSection { section_id: section.id }]).unwrap();
    let late = screen("Late", 390, 844);
    assert!(store
        .apply_for_agent(agent, &tx(&ungrouped, &scope, vec![parse(raw(&late))]))
        .unwrap_err()
        .to_string()
        .contains("no longer exists"));
}

#[test]
fn frozen_section_context_survives_later_selection_and_edits() {
    let (_directory, store, design) = fixture();
    let (design, screens) = with_screens(&store, &design);
    let section = StudioSection { screen_ids: vec![screens[1].id, screens[0].id], ..StudioSection::new("Add images") };
    let design = apply(&store, &design, vec![StudioOperation::CreateSection { section: section.clone() }]).unwrap();
    let agent = Uuid::new_v4();
    store.save_scope(agent, &scope_for_section_request(&design, section.id)).unwrap();
    let renamed = apply(&store, &design, vec![StudioOperation::UpdateSection {
        section_id: section.id,
        name: Some("Renamed later".into()),
        direction: Some(StudioSectionDirection::Vertical),
        gap: None,
        title_style: None,
        header_alignment: None,
    }])
    .unwrap();
    assert_eq!(renamed.manifest.section(section.id).unwrap().name, "Renamed later");
    let context = store.request_context(agent).unwrap();
    assert_eq!(context["scope"]["current_section_id"], serde_json::json!(section.id));
    assert!(context["scope"]["current_screen_id"].is_null());
    assert_eq!(context["current_section"]["name"], "Add images");
    assert_eq!(context["current_section"]["direction"], "horizontal");
    assert_eq!(
        context["current_section"]["screen_ids"],
        serde_json::json!([screens[1].id, screens[0].id])
    );
    assert_eq!(context["current_section"]["screens"][0]["order"], 1);
}

#[test]
fn organization_only_edits_keep_preview_keys_and_need_no_new_reviews() {
    let (_directory, store, design) = fixture();
    let (design, screens) = with_screens(&store, &design);
    let before: Vec<_> = design.manifest.screens.iter().map(|s| store.thumbnail_path(&design, s.id)).collect();
    let agent = Uuid::new_v4();
    let scope = scope_for_request(&design, None, None);
    store.save_scope(agent, &scope).unwrap();
    let section = StudioSection { screen_ids: vec![screens[0].id], ..StudioSection::new("Flow") };
    let grouped = store
        .apply_for_agent(agent, &tx(&design, &scope, vec![
            StudioOperation::CreateSection { section: section.clone() },
            StudioOperation::MoveScreenToSection { screen_id: screens[2].id, section_id: Some(section.id), before_screen_id: None },
            StudioOperation::SetSectionLayout { direction: StudioSectionArrangement::SideBySide, origin: Some(StudioSectionOrigin { x: 10, y: 20 }) },
        ]))
        .unwrap();
    let after: Vec<_> = grouped.manifest.screens.iter().map(|s| store.thumbnail_path(&grouped, s.id)).collect();
    assert_eq!(before, after);
    store.verify_turn_review(agent).unwrap();
}

#[test]
fn geometry_lays_out_mixed_sizes_in_both_directions_without_wrapping() {
    let mut manifest = fixture().2.manifest;
    let phone = screen("Phone", 390, 844);
    let tablet = screen("Tablet", 768, 1024);
    let desktop = screen("Desktop", 1440, 960);
    let mut archived = screen("Old", 390, 844);
    archived.archived = true;
    manifest.screens = vec![phone.clone(), tablet.clone(), desktop.clone(), archived.clone()];
    let row = StudioSection {
        screen_ids: vec![phone.id, archived.id, tablet.id],
        ..StudioSection::new("Row")
    };
    let column = StudioSection {
        screen_ids: vec![desktop.id],
        direction: StudioSectionDirection::Vertical,
        gap: 40,
        ..StudioSection::new("Column")
    };
    let empty = StudioSection::new("Empty");
    manifest.sections = vec![row.clone(), column.clone(), empty.clone()];
    validate_sections(&manifest).unwrap();
    let origin = StudioSectionOrigin { x: 100, y: -50 };
    let board = board_geometry(&manifest, origin);
    assert_eq!(board, board_geometry(&manifest, origin), "deterministic");
    let content_y = (origin.y + SECTION_HEADER + SECTION_PADDING + SCREEN_CAPTION) as f64;
    assert_eq!(board.positions[&phone.id], StudioCanvasPoint { x: (origin.x + SECTION_PADDING) as f64, y: content_y });
    assert_eq!(
        board.positions[&tablet.id],
        StudioCanvasPoint { x: (origin.x + SECTION_PADDING + 390 + DEFAULT_SECTION_GAP as i64) as f64, y: content_y }
    );
    assert!(!board.positions.contains_key(&archived.id), "archived screens take no space");
    let first = &board.sections[0];
    assert_eq!(first.active_screen_ids, vec![phone.id, tablet.id]);
    assert_eq!(first.width, 2 * SECTION_PADDING + 390 + DEFAULT_SECTION_GAP as i64 + 768);
    assert_eq!(first.height, SECTION_HEADER + 2 * SECTION_PADDING + SCREEN_CAPTION + 1024);
    // Stacked: the next section starts below with the section spacing.
    let second = &board.sections[1];
    assert_eq!((second.x, second.y), (origin.x, first.y + first.height + SECTION_SPACING));
    assert_eq!(second.width, 2 * SECTION_PADDING + 1440);
    let third = &board.sections[2];
    assert_eq!((third.width, third.height), (EMPTY_SECTION_WIDTH, SECTION_HEADER + EMPTY_SECTION_HEIGHT));
    // Side by side keeps sections top aligned.
    manifest.section_layout.direction = StudioSectionArrangement::SideBySide;
    let board = board_geometry(&manifest, origin);
    assert_eq!(board.sections[1].y, origin.y);
    assert_eq!(board.sections[1].x, origin.x + board.sections[0].width + SECTION_SPACING);
    // Vertical sections stack screens with caption space and their own gap.
    manifest.sections[0].direction = StudioSectionDirection::Vertical;
    manifest.sections[0].gap = 40;
    let board = board_geometry(&manifest, origin);
    assert_eq!(
        board.positions[&tablet.id].y,
        board.positions[&phone.id].y + 844. + 40. + SCREEN_CAPTION as f64
    );
    assert_eq!(board.positions[&tablet.id].x, board.positions[&phone.id].x);
    // Restoring the archived screen gives it space again.
    manifest.screens[3].archived = false;
    let restored = board_geometry(&manifest, origin);
    assert_eq!(restored.sections[0].active_screen_ids, vec![phone.id, archived.id, tablet.id]);
    let bounds = restored.bounds().unwrap();
    assert_eq!((bounds.0, bounds.1), (origin.x, origin.y));
    // Drop targets resolve the section and the screen to insert before.
    let middle = restored.positions[&archived.id];
    assert_eq!(
        drop_target(&manifest, &restored, desktop.id, middle.x + 10., middle.y + 10.),
        Some((row.id, Some(archived.id)))
    );
    assert_eq!(drop_target(&manifest, &restored, desktop.id, -9_000., -9_000.), None);
}

#[test]
fn maximum_counts_lay_out_quickly() {
    let mut manifest = fixture().2.manifest;
    manifest.screens = (0..200).map(|i| screen(&format!("S{i}"), if i % 2 == 0 { 390 } else { 1440 }, 900)).collect();
    manifest.sections = (0..MAX_SECTIONS)
        .map(|i| StudioSection {
            screen_ids: manifest.screens.iter().skip(i * 2).take(2).map(|s| s.id).collect(),
            ..StudioSection::new(format!("Flow {i} {}", "long title ".repeat(10)))
        })
        .collect();
    validate_sections(&manifest).unwrap();
    let started = std::time::Instant::now();
    let board = board_geometry(&manifest, StudioSectionOrigin::default());
    assert!(started.elapsed() < std::time::Duration::from_millis(50));
    assert_eq!(board.positions.len(), 200);
    assert_eq!(board.sections.len(), MAX_SECTIONS);
}

#[test]
fn canvas_board_origin_is_frozen_below_unsectioned_screens_and_moves_leave_the_board() {
    let (_directory, store, design) = fixture();
    let (design, screens) = with_screens(&store, &design);
    let mut layout = StudioCanvasState::default();
    layout.reconcile_design(&design.manifest);
    let free_positions = layout.positions.clone();
    store.save_canvas_state(design.manifest.id, &layout).unwrap();
    let lowest = design
        .manifest
        .screens
        .iter()
        .map(|s| free_positions[&s.id].y + s.height as f64)
        .fold(0., f64::max);
    let section = StudioSection { screen_ids: vec![screens[0].id], ..StudioSection::new("Flow") };
    let mut grouped = apply(&store, &design, vec![StudioOperation::CreateSection { section: section.clone() }]).unwrap();
    assert!(grouped.manifest.section_layout.origin.is_some(), "Agent creation persists a board origin");
    assert_eq!(grouped.manifest.section_layout.origin.unwrap().y, (lowest + SECTION_SPACING as f64) as i64);
    // Legacy section metadata without an origin still freezes a personal fallback.
    grouped.manifest.section_layout.origin = None;
    layout.reconcile_design(&grouped.manifest);
    let origin = layout.section_origin.expect("fallback origin is frozen");
    assert_eq!(origin.y, (lowest + SECTION_SPACING as f64) as i64);
    // Old free positions are retained, but the board decides grouped screens.
    assert_eq!(layout.positions[&screens[0].id], free_positions[&screens[0].id]);
    let board = layout.board(&grouped.manifest);
    let effective = layout.effective_positions(&board);
    assert_eq!(effective[&screens[0].id], board.positions[&screens[0].id]);
    // Moving a free screen far below does not move the frozen board.
    layout.positions.insert(screens[1].id, StudioCanvasPoint { x: 0., y: 90_000. });
    layout.reconcile_design(&grouped.manifest);
    assert_eq!(layout.board(&grouped.manifest).origin, origin);
    // A screen ungrouped onto the board's area is placed outside it.
    let mut overlapping = grouped.manifest.clone();
    layout.positions.insert(screens[0].id, StudioCanvasPoint { x: origin.x as f64, y: origin.y as f64 });
    overlapping.sections[0].screen_ids.push(screens[2].id);
    overlapping.sections[0].screen_ids.retain(|id| *id != screens[0].id);
    layout.place_outside_board(&overlapping, &[screens[0].id]);
    let placed = layout.positions[&screens[0].id];
    let board = layout.board(&overlapping);
    assert!(board.section_at(placed.x + 1., placed.y + 1.).is_none());
    // Selections are exclusive and stale ones are cleared.
    layout.select_section(Some(section.id));
    assert_eq!(layout.selected_screen_id, None);
    layout.select_screen(Some(screens[1].id));
    assert_eq!(layout.selected_section_id, None);
    layout.select_section(Some(Uuid::new_v4()));
    layout.reconcile_design(&grouped.manifest);
    assert_eq!(layout.selected_section_id, None);
    let encoded: StudioCanvasState = serde_json::from_slice(&serde_json::to_vec(&layout).unwrap()).unwrap();
    assert_eq!(encoded, layout);
    encoded.validate().unwrap();
}

#[test]
fn saved_origin_overrides_the_personal_fallback_and_arrange_keeps_the_board() {
    let (_directory, store, design) = fixture();
    let (design, screens) = with_screens(&store, &design);
    let section = StudioSection { screen_ids: vec![screens[2].id], ..StudioSection::new("Flow") };
    let grouped = apply(&store, &design, vec![
        StudioOperation::CreateSection { section },
        StudioOperation::SetSectionLayout { direction: StudioSectionArrangement::Stacked, origin: Some(StudioSectionOrigin { x: 500, y: 4000 }) },
    ])
    .unwrap();
    let mut layout = StudioCanvasState::default();
    layout.reconcile_design(&grouped.manifest);
    assert_eq!(layout.section_origin, None);
    assert_eq!(layout.board(&grouped.manifest).origin, StudioSectionOrigin { x: 500, y: 4000 });
    layout.arrange_design(&grouped.manifest);
    let board = layout.board(&grouped.manifest);
    assert_eq!(board.origin, StudioSectionOrigin { x: 500, y: 4000 });
    for screen in grouped.manifest.screens.iter().filter(|s| grouped.manifest.section_of(s.id).is_none()) {
        let p = layout.positions[&screen.id];
        assert!(p.y + (screen.height as f64) < 4000., "free screens arrange above the board");
    }
}

#[test]
fn handoffs_carry_section_metadata_for_the_selected_screens() {
    let (_directory, store, design) = fixture();
    let (design, screens) = with_screens(&store, &design);
    let section = StudioSection { screen_ids: vec![screens[0].id, screens[1].id], ..StudioSection::new("Add post") };
    let other = StudioSection { screen_ids: vec![screens[2].id], ..StudioSection::new("Gallery") };
    let design = apply(&store, &design, vec![
        StudioOperation::CreateSection { section: section.clone() },
        StudioOperation::CreateSection { section: other },
    ])
    .unwrap();
    let handoff = store.handoff(design.manifest.id, Some(vec![screens[1].id])).unwrap();
    assert_eq!(handoff.design.manifest.sections.len(), 1);
    assert_eq!(handoff.design.manifest.sections[0].screen_ids, vec![screens[1].id]);
    validate_sections(&handoff.design.manifest).unwrap();
}

#[test]
fn undo_of_a_screen_created_in_a_section_keeps_content_and_redo_restores_it() {
    let (_directory, store, design) = fixture();
    let section = StudioSection::new("Flow");
    let design = apply(&store, &design, vec![StudioOperation::CreateSection { section: section.clone() }]).unwrap();
    let added = screen("Step", 390, 844);
    let created = apply(&store, &design, vec![create(&added, Some(Some(section.id)))]).unwrap();
    assert_eq!(ids(created.manifest.section(section.id).unwrap()), vec![added.id]);
    let undone = store.undo_latest(design.manifest.id).unwrap();
    assert!(undone.manifest.section(section.id).unwrap().screen_ids.is_empty());
    assert!(undone.manifest.screens.iter().any(|s| s.id == added.id && s.archived));
    let redone = store.redo_latest(design.manifest.id).unwrap();
    assert_eq!(ids(redone.manifest.section(section.id).unwrap()), vec![added.id]);
    assert!(redone.manifest.screens.iter().any(|s| s.id == added.id && !s.archived));
}
