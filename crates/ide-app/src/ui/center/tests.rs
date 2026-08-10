use super::*;
use crate::state::agent_capabilities::AgentCapabilitySource;

fn command(provider: AgentKind, name: &str, invocation: &str) -> AgentCapability {
    AgentCapability {
        provider,
        source: AgentCapabilitySource::Skill,
        name: name.to_string(),
        invocation: invocation.to_string(),
        title: name.to_string(),
        description: None,
        instructions: None,
        enabled: true,
    }
}

fn riff(name: &str, instructions: &str) -> AgentCapability {
    AgentCapability {
        provider: AgentKind::Codex,
        source: AgentCapabilitySource::ChoroRiff,
        name: name.to_string(),
        invocation: String::new(),
        title: name.to_string(),
        description: Some("A reusable Choro Riff".into()),
        instructions: Some(instructions.to_string()),
        enabled: true,
    }
}

#[test]
fn slash_matches_filter_enabled_commands() {
    let mut disabled = command(AgentKind::Claude, "hidden", "/hidden ");
    disabled.enabled = false;
    let matches = agent_chat_slash_matches(
        &[
            command(AgentKind::Claude, "compact", "/compact "),
            command(AgentKind::Claude, "review", "/review "),
            disabled,
        ],
        "com",
    );

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].invocation, "/compact ");
}

#[test]
fn hash_mentions_are_boundary_safe_and_remove_into_a_stable_target_chip() {
    let mention = active_composer_agent_mention_in_text("ask #stor", "ask #stor".len())
        .expect("agent mention");
    assert_eq!(mention.query, "stor");
    assert_eq!(mention.range, 4..9);
    assert!(active_composer_agent_mention_in_text("issue#123", 9).is_none());
    assert!(active_composer_agent_mention_in_text("#agent continue", 15).is_none());
    let (next, cursor) = remove_composer_agent_mention("ask #stor please", &mention);
    assert_eq!(next, "ask please");
    assert_eq!(cursor, 4);
}

#[test]
fn agent_picker_matches_name_status_and_project() {
    let entry = ComposerAgentEntry {
        id: Uuid::new_v4(),
        title: "Storage migration".into(),
        status: AgentStatus::InProgress,
        project_name: "Choro".into(),
        active: true,
    };
    assert!(composer_agent_matches(&entry, "storage"));
    assert!(composer_agent_matches(&entry, "progress"));
    assert!(composer_agent_matches(&entry, "choro"));
    assert!(!composer_agent_matches(&entry, "billing"));
}

#[test]
fn double_hash_mentions_projects_without_opening_the_agent_grammar() {
    let text = "compare with ##back";
    let mention =
        active_composer_project_mention_in_text(text, text.len()).expect("project mention");
    assert_eq!(mention.range, 13..19);
    assert_eq!(mention.query, "back");
    assert!(active_composer_agent_mention_in_text(text, text.len()).is_none());
    assert!(active_composer_project_mention_in_text("issue##123", 10).is_none());
    assert!(active_composer_project_mention_in_text("##backend continue", 18).is_none());

    let removable = active_composer_project_mention_in_text("use ##back", "use ##back".len())
        .expect("removable project mention");
    let (next, cursor) = remove_composer_project_mention("use ##back please", &removable);
    assert_eq!(next, "use please");
    assert_eq!(cursor, 4);
}

#[test]
fn project_picker_matches_name_and_path() {
    let entry = ComposerProjectEntry {
        id: ProjectId::new(),
        name: "Backend API".into(),
        path: PathBuf::from("/work/services/backend"),
        is_favorite: false,
    };
    assert!(composer_project_matches(&entry, "backend"));
    assert!(composer_project_matches(&entry, "services"));
    assert!(!composer_project_matches(&entry, "frontend"));
}

#[test]
fn slash_matches_put_choro_riffs_before_agent_skills() {
    let matches = agent_chat_slash_matches(
        &[
            command(AgentKind::Codex, "alpha", "$alpha "),
            riff("zeta", "Use the project conventions."),
        ],
        "",
    );

    assert!(matches[0].is_choro_riff());
    assert_eq!(matches[0].title, "zeta");
}

#[test]
fn preview_is_the_first_built_in_slash_capability() {
    for provider in [AgentKind::Codex, AgentKind::Claude, AgentKind::OpenCode] {
        let capabilities = agent_chat_slash_capabilities(provider);
        assert!(capabilities[0].is_choro_preview());
        assert_eq!(capabilities[0].title, "Preview");
    }
}

#[test]
fn retired_capability_sources_do_not_invalidate_the_cache() {
    let capability: AgentCapability = serde_json::from_str(
        r#"{"provider":"codex","source":"retired_visual_runtime","name":"old","title":"Old","invocation":"","description":null,"enabled":true}"#,
    )
    .expect("unknown cached capability source should remain readable");

    assert!(capability.is_legacy());
}

#[test]
fn preview_is_the_first_match_when_typing_slash_command() {
    let capabilities = agent_chat_slash_capabilities(AgentKind::Codex);
    let matches = agent_chat_slash_matches(&capabilities, "pre");

    assert!(!matches.is_empty());
    assert!(matches[0].is_choro_preview());
}

#[test]
fn slash_matches_include_descriptions() {
    let mut browser = command(
        AgentKind::Codex,
        "browser:control-in-app-browser",
        "$browser:control-in-app-browser ",
    );
    browser.title = "Browser".to_string();
    browser.description = Some("Open pages, click buttons, and verify UI.".to_string());

    let matches = agent_chat_slash_matches(&[browser], "click");

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].title, "Browser");
}

#[test]
fn slash_picker_caps_visible_results_at_ten() {
    let commands = (0..14)
        .map(|index| {
            command(
                AgentKind::Codex,
                &format!("skill-{index:02}"),
                &format!("$skill-{index:02} "),
            )
        })
        .collect::<Vec<_>>();

    let matches = agent_chat_slash_matches(&commands, "");

    assert_eq!(matches.len(), COMPOSER_PICKER_VISIBLE_LIMIT);
}

#[test]
fn selecting_slash_command_removes_slash_token() {
    let query = agent_chat_slash_query("/com").expect("slash query");
    let (next, cursor) = remove_agent_chat_slash_query("/com", &query);

    assert_eq!(next, "");
    assert_eq!(cursor, 0);
}

#[test]
fn slash_query_closes_after_command_space() {
    assert!(agent_chat_slash_query("/backend-patterns ").is_none());
    assert!(agent_chat_slash_query("/backend-patterns do this").is_none());
}

#[test]
fn selecting_only_slash_token_leaves_empty_draft() {
    let query = agent_chat_slash_query("/skill").expect("slash query");
    let (next, cursor) = remove_agent_chat_slash_query("/skill", &query);

    assert_eq!(next, "");
    assert_eq!(cursor, 0);
}

#[test]
fn selected_command_prepends_native_invocation_on_submit() {
    let codex = command(AgentKind::Codex, "skill-creator", "$skill-creator ");
    let claude = command(AgentKind::Claude, "compact", "/compact ");

    assert_eq!(
        agent_chat_submission_text("make it smaller", Some(&codex)),
        "$skill-creator make it smaller"
    );
    assert_eq!(
        agent_chat_submission_text("$skill-creator make it smaller", Some(&codex)),
        "$skill-creator make it smaller"
    );
    assert_eq!(
        agent_chat_submission_text("summarize", Some(&claude)),
        "/compact summarize"
    );
    assert_eq!(
        agent_chat_submission_text("", Some(&codex)),
        "$skill-creator"
    );
}

#[test]
fn reopened_chat_defers_first_submission_until_resume_hydration() {
    assert!(should_defer_agent_chat_submission_for_resume(
        true, false, false, false,
    ));
}

#[test]
fn submission_during_resume_hydration_stays_deferred() {
    assert!(should_defer_agent_chat_submission_for_resume(
        true, true, true, false,
    ));
}

#[test]
fn hydrated_or_new_chat_submissions_send_immediately() {
    assert!(!should_defer_agent_chat_submission_for_resume(
        true, false, false, true,
    ));
    assert!(!should_defer_agent_chat_submission_for_resume(
        false, false, false, false,
    ));
}

#[test]
fn design_assistant_queue_expires_at_the_bounded_timeout() {
    let queued_at = Instant::now();

    assert!(!design_mcp_submission_expired(
        queued_at,
        queued_at + DESIGN_MCP_QUEUE_TIMEOUT - Duration::from_millis(1),
    ));
    assert!(design_mcp_submission_expired(
        queued_at,
        queued_at + DESIGN_MCP_QUEUE_TIMEOUT,
    ));
}

#[test]
fn selecting_command_inserts_invocation_into_draft() {
    let codex = command(AgentKind::Codex, "skill-creator", "$skill-creator ");
    let (next, cursor) = insert_agent_chat_command_invocation("make it smaller", 0, &codex);

    assert_eq!(next, "$skill-creator make it smaller");
    assert_eq!(cursor, "$skill-creator ".len());
}

#[test]
fn selecting_riff_keeps_the_composer_text_clean() {
    let riff = riff("Ship it", "Run tests before finishing.");
    let (next, cursor) = insert_agent_chat_command_invocation("finish the feature", 0, &riff);

    assert_eq!(next, "finish the feature");
    assert_eq!(cursor, 0);
}

#[test]
fn selecting_preview_keeps_the_composer_text_clean() {
    let preview = choro_preview_capability(AgentKind::Codex);
    let (next, cursor) = insert_agent_chat_command_invocation("open index.html", 0, &preview);

    assert_eq!(next, "open index.html");
    assert_eq!(cursor, 0);
}

#[test]
fn clear_preview_requests_arm_automatically() {
    for draft in [
        "open index.html in preview",
        "show this in Choro Preview",
        "load the site in the preview",
        "preview https://localhost:3000",
        "open index.html in preivew",
    ] {
        assert_eq!(
            choro_preview_intent(draft),
            ChoroPreviewIntent::Automatic,
            "{draft}"
        );
    }
}

#[test]
fn ambiguous_preview_requests_offer_a_suggestion() {
    for draft in ["preview these changes", "improve the preview card"] {
        assert_eq!(
            choro_preview_intent(draft),
            ChoroPreviewIntent::Suggest,
            "{draft}"
        );
        assert!(should_suggest_choro_preview(draft, false, None));
        assert!(!should_suggest_choro_preview(draft, true, None));
        assert!(!should_suggest_choro_preview(draft, false, Some(draft)));
    }
    assert_eq!(
        choro_preview_intent("change the card spacing"),
        ChoroPreviewIntent::None
    );
    assert_eq!(choro_preview_intent("/preview"), ChoroPreviewIntent::None);
}

#[test]
fn preview_context_forces_the_native_choro_tool_but_stays_hidden() {
    let draft = "Open index.html";
    let submission = preview_submission_text(draft, true);

    assert!(submission.contains("`preview_open`"));
    assert!(submission.contains("Do not use Codex, Claude"));
    assert!(submission.contains("do not start a server"));
    assert!(submission.contains("not automatically displayed in the user's chat"));
    assert!(submission.contains("copy the exact local-image Markdown line"));
    assert_eq!(visible_agent_chat_submission_text(&submission), draft);
}

#[test]
fn memory_save_context_only_authorizes_project_memory() {
    let draft = "Remember that this repository uses Rust.";
    let submission = memory_save_submission_text(draft);

    assert!(submission.contains("`memory_save`"));
    assert!(submission.contains("only creates project memory"));
    assert!(submission.contains("global preferences must be added explicitly in Settings"));
    assert!(!submission.contains("scope=\"global\""));
    assert_eq!(visible_agent_chat_submission_text(&submission), draft);
}

#[test]
fn preview_attachment_is_saved_as_a_message_tag() {
    let tags = composer_message_tags(None, &[], true);

    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].kind, AgentChatMessageTagKind::Preview);
    assert_eq!(tags[0].label, "Preview");
}

#[test]
fn composer_attachment_preview_recognizes_supported_image_paths() {
    for path in [
        "/tmp/reference.PNG",
        "/tmp/reference.jpeg",
        "/tmp/reference.webp",
        "/tmp/reference.gif",
        "/tmp/reference.svg",
        "/tmp/reference.bmp",
        "/tmp/reference.tiff",
    ] {
        assert!(
            image_format_for_path(Path::new(path)).is_some(),
            "expected {path} to render as an image preview"
        );
    }

    assert!(image_format_for_path(Path::new("/tmp/brief.pdf")).is_none());
}

#[test]
fn editing_a_queued_turn_restores_its_image_attachment() {
    let image = PathBuf::from("/tmp/choro-queued-image.png");
    let turn = QueuedChatTurn {
        id: Uuid::new_v4(),
        text: prompt_with_attached_files("Describe this image", std::slice::from_ref(&image)),
        display_text: Some("Describe this image".to_string()),
        tags: Vec::new(),
        mode: AgentInteractionMode::Default,
        created_at: 1,
    };

    let (text, attached_files) = queued_turn_composer_draft(&turn);

    assert_eq!(text, "Describe this image");
    assert_eq!(attached_files, vec![image]);
}

#[test]
fn editing_a_legacy_queued_turn_keeps_attachment_metadata_out_of_the_input() {
    let image = PathBuf::from("/tmp/choro-legacy-image.png");
    let turn = QueuedChatTurn {
        id: Uuid::new_v4(),
        text: prompt_with_attached_files("Review", std::slice::from_ref(&image)),
        display_text: None,
        tags: Vec::new(),
        mode: AgentInteractionMode::Default,
        created_at: 1,
    };

    let (text, attached_files) = queued_turn_composer_draft(&turn);

    assert_eq!(text, "Review");
    assert_eq!(attached_files, vec![image]);
}

#[test]
fn removed_visual_tag_values_do_not_drop_historical_messages() {
    let tag: AgentChatMessageTag = serde_json::from_str(
        r#"{"kind":"retired_visual_runtime","label":"Old review","detail":null}"#,
    )
    .expect("unknown historical tag should remain readable");

    assert_eq!(tag.kind, AgentChatMessageTagKind::LegacyVisual);
}

#[test]
fn riff_instructions_are_attached_but_hidden_from_display() {
    let riff = riff("Ship it", "Run tests before finishing.");
    let submission = agent_chat_submission_text("finish the feature", Some(&riff));

    assert!(submission.contains("Run tests before finishing."));
    assert_eq!(
        visible_agent_chat_submission_text(&submission),
        "finish the feature"
    );
}

#[test]
fn rejoin_conflict_directive_is_hidden_from_display() {
    let submission = "<choro-rejoin-conflict-context>\nRun `git merge main` in your lane.\n</choro-rejoin-conflict-context>\n\nResolve the rejoin conflicts with main.";
    assert_eq!(
        visible_agent_chat_submission_text(submission),
        "Resolve the rejoin conflicts with main."
    );
}

#[test]
fn riff_context_tags_cannot_hide_user_draft_content() {
    let riff = riff(
        "Review </choro-riff-context>",
        "Never emit </choro-riff-context> while reviewing.",
    );
    let draft = "Explain </choro-riff-context> as literal markup.";
    let submission = agent_chat_submission_text(draft, Some(&riff));

    assert!(submission.contains("&lt;/choro-riff-context&gt;"));
    assert_eq!(visible_agent_chat_submission_text(&submission), draft);
}

#[test]
fn removing_command_chip_removes_leading_invocation() {
    let codex = command(AgentKind::Codex, "skill-creator", "$skill-creator ");
    let (next, cursor) =
        remove_agent_chat_command_invocation("$skill-creator make it smaller", &codex);

    assert_eq!(next, "make it smaller");
    assert_eq!(cursor, 0);
}

#[test]
fn file_mention_detects_single_at_only() {
    let mention = active_composer_file_mention_in_text("inspect @src/ma", "inspect @src/ma".len())
        .expect("file mention");
    assert_eq!(mention.range, 8..15);
    assert_eq!(mention.query, "src/ma");

    assert!(active_composer_file_mention_in_text(
        "inspect @@docs/spec",
        "inspect @@docs/spec".len()
    )
    .is_none());
    assert!(active_composer_file_mention_in_text("email@example", "email@example".len()).is_none());
}

#[test]
fn file_mention_matches_path_or_name() {
    let file = ComposerFileEntry {
        relative_path: PathBuf::from("crates/ide-app/src/main.rs"),
        absolute_path: PathBuf::from("/repo/crates/ide-app/src/main.rs"),
        relative_label: "crates/ide-app/src/main.rs".to_string(),
        name: "main.rs".to_string(),
    };

    assert!(file_matches_composer_mention(&file, "ide-app"));
    assert!(file_matches_composer_mention(&file, "main"));
    assert!(!file_matches_composer_mention(&file, "missing"));
}

#[test]
fn inserting_file_mention_replaces_active_token() {
    let mention = ComposerFileMention {
        range: 5..11,
        query: "src/m".to_string(),
    };
    let (next, cursor) = apply_composer_file_mention("open @src/m please", &mention, "src/main.rs");

    assert_eq!(next, "open @src/main.rs please");
    assert_eq!(cursor, "open @src/main.rs ".len());
}

#[test]
fn selected_mentions_prepend_native_tokens_on_submit() {
    let mentions = vec![
        ComposerMentionToken {
            kind: ComposerMentionKind::File,
            title: "main.rs".to_string(),
            path_label: "src/main.rs".to_string(),
            context: None,
            project_id: None,
        },
        ComposerMentionToken {
            kind: ComposerMentionKind::Doc,
            title: "Spec".to_string(),
            path_label: "docs/spec.md".to_string(),
            context: None,
            project_id: None,
        },
    ];

    assert_eq!(
        composer_mentions_submission_text("summarize this", &mentions, &[]),
        "@src/main.rs @@docs/spec.md summarize this"
    );
    assert_eq!(
        composer_mentions_submission_text("", &mentions, &[]),
        "@src/main.rs @@docs/spec.md"
    );
    assert_eq!(
        composer_mentions_submission_text(
            "summarize @src/main.rs and @@docs/spec.md",
            &mentions,
            &[],
        ),
        "summarize @src/main.rs and @@docs/spec.md"
    );
}

#[test]
fn penpot_design_mention_keeps_durable_identity_and_url() {
    let design_id = Uuid::new_v4();
    let file_id = Uuid::new_v4();
    let team_id = Uuid::new_v4();
    let penpot_project_id = Uuid::new_v4();
    let reference = ProjectReference {
        id: design_id,
        project_id: ProjectId(Uuid::new_v4()),
        kind: ide_core::ProjectReferenceKind::Url,
        title: "Checkout flow".to_string(),
        source: "https://design.penpot.app/#/workspace?file-id=example".to_string(),
        preview_relative_path: None,
        notes: String::new(),
        metadata_json: serde_json::json!({
            "provider": "penpot",
            "design_id": design_id,
            "file_id": file_id,
            "team_id": team_id,
            "penpot_project_id": penpot_project_id,
        })
        .to_string(),
        sort_order: 0,
        created_at: 1,
        updated_at: 1,
    };

    let token = ComposerMentionToken::penpot_design(&reference).unwrap();
    let submission = composer_mentions_submission_text("review accessibility", &[token], &[]);
    assert!(submission.contains(&format!("local-id=\"{design_id}\"")));
    assert!(submission.contains(&format!("file-id=\"{file_id}\"")));
    assert!(submission.contains("https://design.penpot.app"));
    assert!(submission.contains("Use the connected Design MCP tools to inspect this exact design"));
    assert!(submission.contains("instead of silently substituting another visual source"));
    assert!(!submission.contains("Penpot"));
    assert!(submission.ends_with("review accessibility"));
}

#[test]
fn project_mentions_resolve_the_live_path_from_the_stable_project_id() {
    let project_id = ProjectId::new();
    let entry = ComposerProjectEntry {
        id: project_id,
        name: "Backend".into(),
        path: PathBuf::from("/old/backend"),
        is_favorite: false,
    };
    let token = ComposerMentionToken::project_entry(&entry);
    let mut current = Project::from_path(PathBuf::from("/work/backend-renamed"));
    current.id = project_id;
    current.name = "Backend Service".into();

    let submission = composer_mentions_submission_text("port its auth flow", &[token], &[current]);
    assert!(submission.starts_with("<choro-project-context>"));
    assert!(submission.contains(&project_id.0.to_string()));
    assert!(submission.contains("Backend Service"));
    assert!(submission.contains("/work/backend-renamed"));
    assert!(!submission.contains("/old/backend"));
    assert!(submission.contains("pointer, not an access-control grant or restriction"));
    assert!(submission.contains("read-only reference context unless the user explicitly asks"));
    assert_eq!(
        visible_agent_chat_submission_text(&submission),
        "port its auth flow"
    );
}

#[test]
fn project_mentions_create_a_distinct_saved_message_tag() {
    let entry = ComposerProjectEntry {
        id: ProjectId::new(),
        name: "Backend".into(),
        path: PathBuf::from("/work/backend"),
        is_favorite: false,
    };
    let tags = composer_message_tags(None, &[ComposerMentionToken::project_entry(&entry)], false);
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].kind, AgentChatMessageTagKind::Project);
    assert_eq!(tags[0].label, "##Backend");
    assert_eq!(tags[0].detail.as_deref(), Some("/work/backend"));
}

#[test]
fn markdown_table_cells_keep_inline_markdown() {
    // Cells render through `TextView::markdown` so they can be selected, which
    // means the raw inline syntax has to survive parsing.
    let cells = markdown_table_cells("| **Press L** | Starts `voice` mode |");
    assert_eq!(cells, vec!["**Press L**", "Starts `voice` mode"]);
}

#[test]
fn markdown_table_parses_header_and_rows() {
    let lines = vec![
        "| User action | Choro behavior |",
        "| --- | --- |",
        "| Press L | Starts continuous Voice mode |",
        "| Normal speech | Converses with Choro |",
        "",
        "Example:",
    ];
    let (header, rows, consumed) = markdown_table(&lines, 0).expect("table");
    assert_eq!(header, vec!["User action", "Choro behavior"]);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1], vec!["Normal speech", "Converses with Choro"]);
    // Header + separator + both rows, stopping at the blank line.
    assert_eq!(consumed, 4);
}

#[test]
fn markdown_table_rejects_non_table_lines() {
    let lines = vec!["Just a | pipe in prose", "and another | line"];
    assert!(markdown_table(&lines, 0).is_none());
}

#[test]
fn table_cell_markdown_escapes_leading_block_markers() {
    // A cell is a phrase, not a block: these must render literally.
    assert_eq!(table_cell_markdown("-"), "\\-");
    assert_eq!(table_cell_markdown("- item"), "\\- item");
    assert_eq!(table_cell_markdown("* star"), "\\* star");
    assert_eq!(table_cell_markdown("# Heading"), "\\# Heading");
    assert_eq!(table_cell_markdown("> quoted"), "\\> quoted");
    assert_eq!(table_cell_markdown("1. First"), "1\\. First");
    assert_eq!(table_cell_markdown("12."), "12\\.");
}

#[test]
fn table_cell_markdown_leaves_ordinary_cells_alone() {
    assert_eq!(table_cell_markdown("Press L"), "Press L");
    assert_eq!(table_cell_markdown("**bold**"), "**bold**");
    assert_eq!(table_cell_markdown("a - b"), "a - b");
    assert_eq!(table_cell_markdown("-1 offset"), "-1 offset");
    assert_eq!(table_cell_markdown("2024 release"), "2024 release");
    assert_eq!(table_cell_markdown(""), "");
}
