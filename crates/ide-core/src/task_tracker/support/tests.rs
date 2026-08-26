use super::*;
use serde_json::json;

#[test]
fn pr_done_status_round_trips_through_provider_config() {
    let connection =
        TaskTrackerConnection::new_external(IssueTrackerProvider::ClickUp, "Team", "", "token");
    assert_eq!(connection.pr_done_status(), None);

    let with_status = connection.with_pr_done_status(Some("  In Review  ".to_string()));
    // Trimmed on write, and the original is left untouched (immutability).
    assert_eq!(with_status.pr_done_status().as_deref(), Some("In Review"));
    assert_eq!(connection.pr_done_status(), None);

    // Clearing removes the key again.
    let cleared = with_status.with_pr_done_status(None);
    assert_eq!(cleared.pr_done_status(), None);

    // Blank/whitespace is treated as "no suggestion".
    let blank = connection.with_pr_done_status(Some("   ".to_string()));
    assert_eq!(blank.pr_done_status(), None);
}

#[test]
fn pr_done_status_preserves_other_config_keys() {
    let mut connection =
        TaskTrackerConnection::new_external(IssueTrackerProvider::Linear, "Team", "", "token");
    connection.provider_config_json = json!({ "keep": "me" }).to_string();
    let updated = connection.with_pr_done_status(Some("Done".to_string()));
    let parsed: Value = serde_json::from_str(&updated.provider_config_json).unwrap();
    assert_eq!(parsed.get("keep").and_then(Value::as_str), Some("me"));
    assert_eq!(
        parsed.get("pr_done_status").and_then(Value::as_str),
        Some("Done")
    );
}

#[test]
fn pocketcomet_snapshot_filters_tasks_by_saved_assignee_identity() {
    let snapshot = PocketCometTaskSourceSnapshot {
        device_id: "device-1".into(),
        workspace_id: "workspace-1".into(),
        pocketcomet_project_id: "project-1".into(),
        project_name: "Website".into(),
        statuses: vec![
            PocketCometTaskStatus {
                id: "started".into(),
                name: "In progress".into(),
                category: "started".into(),
            },
            PocketCometTaskStatus {
                id: "done".into(),
                name: "Done".into(),
                category: "completed".into(),
            },
            PocketCometTaskStatus {
                id: "review".into(),
                name: "Review".into(),
                category: "started".into(),
            },
        ],
        assignees: vec![
            PocketCometTaskAssignee {
                id: "user-ada".into(),
                name: "Ada".into(),
                email: Some("ada@example.com".into()),
            },
            PocketCometTaskAssignee {
                id: "user-grace".into(),
                name: "Grace".into(),
                email: None,
            },
        ],
        tasks: vec![
            PocketCometTask {
                id: "task-ada".into(),
                title: "Design settings".into(),
                description: "Keep the filter in Choro.".into(),
                status_id: "started".into(),
                status: "In progress".into(),
                status_category: "started".into(),
                list_id: "list-1".into(),
                list_name: "Tasks".into(),
                assignee_id: Some("user-ada".into()),
                assignee_name: Some("Ada".into()),
                priority: "high".into(),
                labels: vec!["desktop".into()],
                comments: vec![PocketCometTaskComment {
                    id: "comment-1".into(),
                    author_name: "Ada".into(),
                    body: "Ready for review.".into(),
                    created_at: 2,
                }],
                attachments: vec![PocketCometTaskAttachment {
                    id: "attachment-1".into(),
                    file_name: "mockup.png".into(),
                    mime_type: Some("image/png".into()),
                    size_bytes: 1_234,
                    asset_file: format!("pocketcomet-{}.png", "a".repeat(64)),
                }],
                created_at: 1,
                updated_at: 2,
            },
            PocketCometTask {
                id: "task-grace".into(),
                title: "Ship sync".into(),
                description: String::new(),
                status_id: "done".into(),
                status: "Done".into(),
                status_category: "completed".into(),
                list_id: "list-1".into(),
                list_name: "Tasks".into(),
                assignee_id: Some("user-grace".into()),
                assignee_name: Some("Grace".into()),
                priority: "none".into(),
                labels: Vec::new(),
                comments: Vec::new(),
                attachments: Vec::new(),
                created_at: 1,
                updated_at: 3,
            },
        ],
    };
    let mut connection = TaskTrackerConnection::new_external(
        IssueTrackerProvider::PocketComet,
        "PocketComet",
        "",
        "",
    );
    connection.provider_config_json = serde_json::to_string(&snapshot).unwrap();
    connection.source_id = Some("project-1".into());
    connection.assignee_account_id = Some("user-ada".into());
    connection.assignee_display_name = Some("Ada".into());

    let client = TaskTrackerClient::new(connection).unwrap();
    let board = client.load_board().unwrap();

    assert_eq!(board.issues.len(), 1);
    assert_eq!(board.issues[0].reference.issue_id, "task-ada");
    assert_eq!(board.issues[0].labels, vec!["desktop"]);
    assert_eq!(
        board.columns.len(),
        3,
        "empty project statuses stay visible"
    );
    assert_eq!(client.list_assignees().unwrap().len(), 2);

    let detail = client
        .load_task_detail(&board.issues[0].reference, &board.columns)
        .expect("PocketComet task detail");
    assert_eq!(detail.attachments.len(), 1);
    assert_eq!(detail.attachments[0].filename, "mockup.png");
    assert_eq!(detail.attachments[0].size, Some(1_234));
    assert_eq!(detail.comments.len(), 1);
    assert_eq!(detail.comments[0].author, "Ada");
    assert_eq!(detail.comments[0].body.text, "Ready for review.");
    assert_eq!(
        client
            .available_statuses(&board.issues[0].reference)
            .unwrap()
            .len(),
        3
    );
}

#[test]
fn pocketcomet_task_actions_use_a_stable_tagged_wire_shape() {
    let action = PocketCometTaskAction {
        action_id: "00000000-0000-0000-0000-000000000123".into(),
        device_id: "device-1".into(),
        workspace_id: "workspace-1".into(),
        pocketcomet_project_id: "project-1".into(),
        task_id: "task-1".into(),
        created_at: 1,
        command: PocketCometTaskActionCommand::SetStatus {
            status_id: "status-done".into(),
        },
    };

    let wire = serde_json::to_value(&action).unwrap();
    assert_eq!(wire["kind"], "set_status");
    assert_eq!(wire["status_id"], "status-done");
    assert_eq!(
        serde_json::from_value::<PocketCometTaskAction>(wire).unwrap(),
        action
    );
}

#[test]
fn personal_task_status_id_round_trips() {
    for status in PersonalTaskStatus::ALL {
        assert_eq!(
            PersonalTaskStatus::from_status_id(status.status_id()),
            Some(status)
        );
    }
    assert_eq!(PersonalTaskStatus::from_status_id("nope"), None);

    let options = PersonalTaskStatus::options();
    assert_eq!(options.len(), 3);
    assert_eq!(options[0].name, "To Do");
    assert_eq!(options[2].apply_id, "done");
}

#[test]
fn linear_state_category_maps_types() {
    assert_eq!(linear_state_category("completed".to_string()), "done");
    assert_eq!(linear_state_category("canceled".to_string()), "done");
    assert_eq!(linear_state_category("started".to_string()), "in progress");
    assert_eq!(linear_state_category("backlog".to_string()), "new");
    assert_eq!(linear_state_category("unstarted".to_string()), "new");
}

#[test]
fn adf_to_text_handles_basic_blocks() {
    let adf = json!({
        "type": "doc",
        "content": [
            {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "Goal"}]},
            {"type": "paragraph", "content": [{"type": "text", "text": "Build it"}, {"type": "hardBreak"}, {"type": "text", "text": "Carefully"}]},
            {"type": "bulletList", "content": [
                {"type": "listItem", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "First"}]}]},
                {"type": "listItem", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "Second"}]}]}
            ]}
        ]
    });

    let text = jira_adf_to_text(&adf);
    assert!(text.contains("## Goal"));
    assert!(text.contains("Build it\nCarefully"));
    assert!(text.contains("- First"));
    assert!(text.contains("- Second"));
}

#[test]
fn adf_to_rich_text_maps_media_to_image_attachment() {
    let adf = json!({
        "type": "doc",
        "content": [
            {"type": "paragraph", "content": [{"type": "text", "text": "Use this mockup:"}]},
            {"type": "mediaSingle", "content": [
                {"type": "media", "attrs": {"id": "media-1", "type": "file", "alt": "spec.png"}}
            ]},
            {"type": "paragraph", "content": [{"type": "text", "text": "Keep spacing tight."}]}
        ]
    });
    let attachments = vec![TaskAttachment {
        id: "10001".to_string(),
        filename: "spec.png".to_string(),
        mime_type: Some("image/png".to_string()),
        content_url: Some(
            "https://example.atlassian.net/secure/attachment/10001/spec.png".to_string(),
        ),
        thumbnail_url: None,
        local_path: Some(PathBuf::from("/tmp/spec.png")),
        size: Some(42),
    }];

    let rich_text = jira_adf_to_rich_text(&adf, &attachments);

    assert!(rich_text.text.contains("[image: spec.png]"));
    assert!(rich_text.blocks.iter().any(|block| {
        matches!(
            block,
            TaskContentBlock::Image(image)
                if image.attachment_id.as_deref() == Some("10001")
                    && image.local_path.as_deref() == Some(Path::new("/tmp/spec.png"))
        )
    }));
}

#[test]
fn task_ref_matches_by_provider_site_and_key() {
    let left = TaskRef {
        provider: IssueTrackerProvider::Jira,
        site_url: "https://example.atlassian.net/".into(),
        issue_id: "1".into(),
        issue_key: "ABC-1".into(),
        issue_url: "https://example.atlassian.net/browse/ABC-1".into(),
        title: "A".into(),
    };
    let right = TaskRef {
        issue_key: "abc-1".into(),
        site_url: "https://example.atlassian.net".into(),
        ..left.clone()
    };
    assert!(left.same_issue(&right));
}

#[test]
fn assignee_filter_builds_jira_jql() {
    assert_eq!(
        jira_assignee_filter_to_jql("currentUser()").as_deref(),
        Some("assignee = currentUser()")
    );
    assert_eq!(
        jira_assignee_filter_to_jql("Ada Lovelace").as_deref(),
        Some("assignee = \"Ada Lovelace\"")
    );
    assert_eq!(
        jira_assignee_filter_to_jql("assignee is EMPTY").as_deref(),
        Some("assignee is EMPTY")
    );
    assert!(jira_assignee_filter_to_jql("   ").is_none());
}

#[test]
fn jira_assignee_account_id_builds_jql_and_label() {
    let mut connection = TaskTrackerConnection::new_jira(
        "Jira",
        "https://example.atlassian.net/",
        "dev@example.com",
        "${JIRA_API_TOKEN}",
    );
    connection.assignee_filter = Some("currentUser()".to_string());
    connection.assignee_account_id = Some("712020:abc-123".to_string());
    connection.assignee_display_name = Some("Ada Lovelace".to_string());

    assert_eq!(
        connection.assignee_filter_jql().as_deref(),
        Some("assignee = \"712020:abc-123\"")
    );
    assert_eq!(connection.assignee_label(), Some("Ada Lovelace"));
}

#[test]
fn maps_jira_user_payload() {
    let value = json!({
        "accountId": "712020:abc-123",
        "displayName": "Ada Lovelace",
        "emailAddress": "ada@example.com",
        "active": true,
        "avatarUrls": {
            "24x24": "https://avatar.example/24.png"
        }
    });

    let user = jira_user_from_value(&value).unwrap();

    assert_eq!(user.account_id, "712020:abc-123");
    assert_eq!(user.display_name, "Ada Lovelace");
    assert_eq!(user.email.as_deref(), Some("ada@example.com"));
    assert_eq!(
        user.avatar_url.as_deref(),
        Some("https://avatar.example/24.png")
    );
    assert!(user.active);
}

#[test]
fn maps_jira_issue_payload_to_task_summary() {
    let mut connection = TaskTrackerConnection::new_jira(
        "Jira",
        "https://example.atlassian.net/",
        "dev@example.com",
        "${JIRA_API_TOKEN}",
    );
    connection.board_id = Some(42);
    let client = JiraClient::new(connection).unwrap();
    let columns = vec![TaskBoardColumn {
        name: "Doing".to_string(),
        status_ids: vec!["3".to_string()],
    }];
    let issue = json!({
        "id": "10001",
        "key": "APP-123",
        "fields": {
            "summary": "Add task board",
            "status": {
                "id": "3",
                "name": "In Progress",
                "statusCategory": { "name": "In Progress" }
            },
            "assignee": { "displayName": "Ada Lovelace" },
            "priority": { "name": "High" },
            "issuetype": { "name": "Story" },
            "labels": ["ide", "jira"],
            "updated": "2026-07-04T10:00:00.000+0000",
            "created": "2026-07-01T10:00:00.000+0000"
        }
    });

    let summary = client.issue_summary_from_value(&issue, &columns).unwrap();
    assert_eq!(summary.reference.issue_key, "APP-123");
    assert_eq!(
        summary.reference.issue_url,
        "https://example.atlassian.net/browse/APP-123"
    );
    assert_eq!(summary.column, "Doing");
    assert_eq!(summary.assignee.as_deref(), Some("Ada Lovelace"));
    assert_eq!(summary.priority.as_deref(), Some("High"));
    assert_eq!(summary.issue_type.as_deref(), Some("Story"));
    assert_eq!(summary.labels, vec!["ide", "jira"]);
}

#[test]
fn maps_linear_issue_payload_to_task_summary() {
    let issue = json!({
        "id": "lin-id",
        "identifier": "ENG-12",
        "title": "Ship task source",
        "url": "https://linear.app/acme/issue/ENG-12/ship-task-source",
        "priorityLabel": "High",
        "createdAt": "2026-07-01T10:00:00.000Z",
        "updatedAt": "2026-07-04T10:00:00.000Z",
        "state": {"id": "state-1", "name": "In Progress", "type": "started"},
        "assignee": {"name": "Ada Lovelace"},
        "labels": {"nodes": [{"name": "tasks"}]}
    });

    let summary = linear_issue_summary(&issue).unwrap();

    assert_eq!(summary.reference.provider, IssueTrackerProvider::Linear);
    assert_eq!(summary.reference.issue_key, "ENG-12");
    assert_eq!(summary.status, "In Progress");
    assert_eq!(summary.assignee.as_deref(), Some("Ada Lovelace"));
    assert_eq!(summary.labels, vec!["tasks"]);
}

#[test]
fn maps_clickup_task_payload_to_task_summary() {
    let mut connection = TaskTrackerConnection::new_external(
        IssueTrackerProvider::ClickUp,
        "ClickUp",
        "https://api.clickup.com",
        "${CLICKUP_API_TOKEN}",
    );
    connection.source_id = Some("list-1".to_string());
    let task = json!({
        "id": "cu-1",
        "name": "ClickUp task",
        "url": "https://app.clickup.com/t/cu-1",
        "status": {"status": "to do", "type": "open"},
        "priority": {"priority": "urgent"},
        "assignees": [{"username": "Ada"}],
        "tags": [{"name": "mvp"}],
        "date_created": "1",
        "date_updated": "2"
    });

    let summary = clickup_task_summary(&task, &connection).unwrap();

    assert_eq!(summary.reference.provider, IssueTrackerProvider::ClickUp);
    assert_eq!(summary.reference.issue_key, "cu-1");
    assert_eq!(summary.column, "to do");
    assert_eq!(summary.priority.as_deref(), Some("urgent"));
    assert_eq!(summary.labels, vec!["mvp"]);
}

#[test]
fn maps_asana_task_payload_to_task_summary() {
    let mut connection = TaskTrackerConnection::new_external(
        IssueTrackerProvider::Asana,
        "Asana",
        "https://app.asana.com",
        "${ASANA_API_TOKEN}",
    );
    connection.source_id = Some("project-1".to_string());
    let task = json!({
        "gid": "120",
        "name": "Asana task",
        "completed": false,
        "permalink_url": "https://app.asana.com/0/project/120",
        "memberships": [{"section": {"name": "In Progress"}}],
        "assignee": {"name": "Ada Lovelace"},
        "tags": [{"name": "spec"}],
        "created_at": "2026-07-01T10:00:00.000Z",
        "modified_at": "2026-07-04T10:00:00.000Z"
    });

    let summary = asana_task_summary(&task, &connection).unwrap();

    assert_eq!(summary.reference.provider, IssueTrackerProvider::Asana);
    assert_eq!(summary.reference.issue_key, "120");
    assert_eq!(summary.column, "In Progress");
    assert_eq!(summary.assignee.as_deref(), Some("Ada Lovelace"));
    assert_eq!(summary.labels, vec!["spec"]);
}

#[test]
fn personal_task_ref_matches_project_local_task() {
    let project_id = ProjectId(Uuid::parse_str("00000000-0000-0000-0000-000000000123").unwrap());
    let task = PersonalTaskRecord {
        id: Uuid::parse_str("00000000-0000-0000-0000-000000000456").unwrap(),
        project_id,
        key_number: 7,
        title: "Local spec".to_string(),
        description_markdown: "Build local flow".to_string(),
        status: PersonalTaskStatus::Todo,
        priority: PersonalTaskPriority::Medium,
        labels: vec!["local".to_string()],
        created_at: 1,
        updated_at: 2,
        archived: false,
    };

    let reference = task.task_ref();

    assert_eq!(reference.provider, IssueTrackerProvider::Personal);
    assert_eq!(reference.issue_key, "TASK-7");
    assert!(reference.same_issue(&TaskRef {
        issue_key: "task-7".to_string(),
        ..reference.clone()
    }));
}

#[test]
fn task_connection_diagnostics_and_debug_hide_token() {
    let connection = TaskTrackerConnection::new_external(
        IssueTrackerProvider::Linear,
        "Linear",
        "https://api.linear.app",
        "literal-super-secret",
    );

    let diagnostic = connection.redact_diagnostic(
        "remote echoed literal-super-secret; Authorization: Bearer another-token-123",
    );
    let debug = format!("{connection:?}");

    assert!(!diagnostic.contains("literal-super-secret"));
    assert!(!diagnostic.contains("another-token-123"));
    assert!(diagnostic.matches("[REDACTED]").count() >= 2);
    assert!(!debug.contains("literal-super-secret"));
    assert!(debug.contains("[REDACTED]"));
}
