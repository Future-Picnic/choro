use super::*;

pub(super) fn jsonrpc_id_key(id: &Value) -> String {
    match id {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Null => next_request_id(),
        other => other.to_string(),
    }
}

pub(super) fn pending_user_input_from_codex_request(
    jsonrpc_id: String,
    params: &Value,
) -> PendingUserInput {
    let questions = params
        .get("questions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|question| {
            let id = question
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("question")
                .to_string();
            let header = question
                .get("header")
                .and_then(Value::as_str)
                .unwrap_or("Pick one")
                .to_string();
            let question_text = question
                .get("question")
                .and_then(Value::as_str)
                .unwrap_or("Choose an option")
                .to_string();
            let options = question
                .get("options")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|option| {
                    PendingUserInputOption::new(
                        option
                            .get("label")
                            .and_then(Value::as_str)
                            .unwrap_or("Option"),
                        option
                            .get("description")
                            .and_then(Value::as_str)
                            .unwrap_or_default(),
                    )
                })
                .collect::<Vec<_>>();
            PendingUserInputQuestion::pick_one(id, header, question_text, options)
        })
        .collect::<Vec<_>>();
    PendingUserInput::new(jsonrpc_id, questions)
}

pub(super) fn pending_approval_from_codex_request(
    request_id: String,
    method: &str,
    params: &Value,
) -> PendingApproval {
    let reason = params
        .get("reason")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let (kind, title, detail) = match method {
        "item/commandExecution/requestApproval" => {
            let command = params
                .get("command")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty());
            let cwd = params
                .get("cwd")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty());
            let detail = [command, reason, cwd.map(|value| value)]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join("\n");
            (
                PendingApprovalKind::Command,
                "Allow command execution?",
                (!detail.is_empty()).then(|| ide_core::redact_sensitive_text(&detail)),
            )
        }
        "item/fileChange/requestApproval" => {
            let root = params
                .get("grantRoot")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty());
            let detail = [reason, root]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join("\n");
            (
                PendingApprovalKind::FileChange,
                "Allow file changes?",
                (!detail.is_empty()).then(|| ide_core::redact_sensitive_text(&detail)),
            )
        }
        _ => {
            let permissions = params.get("permissions").unwrap_or(&Value::Null);
            let mut requested = Vec::new();
            if permissions
                .pointer("/network/enabled")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                requested.push("Network access".to_string());
            }
            let file_count = permissions
                .pointer("/fileSystem/entries")
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or(0)
                + permissions
                    .pointer("/fileSystem/read")
                    .and_then(Value::as_array)
                    .map(Vec::len)
                    .unwrap_or(0)
                + permissions
                    .pointer("/fileSystem/write")
                    .and_then(Value::as_array)
                    .map(Vec::len)
                    .unwrap_or(0);
            if file_count > 0 {
                requested.push(format!(
                    "File access to {file_count} path{}",
                    if file_count == 1 { "" } else { "s" }
                ));
            }
            if let Some(reason) = reason {
                requested.push(reason.to_string());
            }
            let detail = requested.join("\n");
            (
                PendingApprovalKind::Permissions,
                "Grant additional permissions?",
                (!detail.is_empty()).then(|| ide_core::redact_sensitive_text(&detail)),
            )
        }
    };
    PendingApproval::new(request_id, kind, title, detail)
}

pub(super) fn pending_user_input_from_bridge_event(event: &Value) -> PendingUserInput {
    let request_id = event
        .get("request_id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(next_request_id);
    let questions = event
        .get("questions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|question| {
            let id = question
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("question")
                .to_string();
            let header = question
                .get("header")
                .and_then(Value::as_str)
                .unwrap_or("Pick one")
                .to_string();
            let question_text = question
                .get("question")
                .and_then(Value::as_str)
                .unwrap_or("Choose an option")
                .to_string();
            let options = question
                .get("options")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|option| {
                    PendingUserInputOption::new(
                        option
                            .get("label")
                            .and_then(Value::as_str)
                            .unwrap_or("Option"),
                        option
                            .get("description")
                            .and_then(Value::as_str)
                            .unwrap_or_default(),
                    )
                })
                .collect::<Vec<_>>();
            if question
                .get("multi_select")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                PendingUserInputQuestion::pick_many(id, header, question_text, options)
            } else {
                PendingUserInputQuestion::pick_one(id, header, question_text, options)
            }
        })
        .collect::<Vec<_>>();
    PendingUserInput::new(request_id, questions)
}

pub(super) fn work_log_from_bridge_event(event: &Value) -> Option<WorkLogEntry> {
    let id = event
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(next_request_id);
    let collapse_key = event
        .get("collapse_key")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| id.clone());
    let title = event
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("Tool call");
    let kind = match event.get("kind").and_then(Value::as_str) {
        Some("command") => WorkLogEntryKind::Command,
        Some("plan") => WorkLogEntryKind::Plan,
        Some("user_input") => WorkLogEntryKind::UserInput,
        Some("system") => WorkLogEntryKind::System,
        Some("step") => WorkLogEntryKind::Step,
        _ => WorkLogEntryKind::Tool,
    };
    let status = match event.get("status").and_then(Value::as_str) {
        Some("pending") => WorkLogStatus::Pending,
        Some("in_progress") => WorkLogStatus::InProgress,
        Some("failed") => WorkLogStatus::Failed,
        _ => WorkLogStatus::Completed,
    };
    let detail = event
        .get("detail")
        .and_then(Value::as_str)
        .map(str::to_string);
    Some(WorkLogEntry::new(id, collapse_key, kind, title, status).detail(detail))
}

pub(super) fn item_id_from_params(params: &Value) -> Option<String> {
    params
        .get("itemId")
        .or_else(|| params.get("item_id"))
        .or_else(|| params.pointer("/item/id"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

pub(super) fn codex_compaction_activity(method: &str, params: &Value) -> Option<bool> {
    // Older app servers also send this completion-only notification.
    if method == "thread/compacted" {
        return Some(false);
    }
    if item_type_from_params(params) != Some("contextCompaction") {
        return None;
    }
    match method {
        "item/started" => Some(true),
        "item/completed" => Some(false),
        _ => None,
    }
}

pub(super) fn codex_notification_for_thread(params: &Value, active_thread: Option<&str>) -> bool {
    let source_thread = params
        .get("threadId")
        .or_else(|| params.get("thread_id"))
        .or_else(|| params.get("thread").and_then(|thread| thread.get("id")))
        .and_then(Value::as_str);
    match (active_thread, source_thread) {
        (Some(active), Some(source)) => active == source,
        // Connection-wide notifications and the initial handshake have no
        // thread scope. Do not reject those as if they were child output.
        _ => true,
    }
}

#[cfg(test)]
mod stream_routing_tests {
    use super::*;

    #[test]
    fn delegated_text_and_lifecycle_notifications_are_filtered_by_thread() {
        for method in [
            "item/agentMessage/delta",
            "item/completed",
            "turn/completed",
            "thread/tokenUsage/updated",
            "error",
        ] {
            let child = json!({"method": method, "params": {"threadId": "reviewer", "itemId": "reply", "delta": "review text"}});
            assert!(
                !codex_notification_for_thread(&child["params"], Some("parent")),
                "{method}"
            );
            assert!(
                codex_notification_for_thread(&child["params"], Some("reviewer")),
                "{method}"
            );
        }
    }

    #[test]
    fn thread_objects_and_legacy_ids_also_preserve_conversation_scope() {
        for params in [
            json!({"thread": {"id": "child"}}),
            json!({"thread_id": "child"}),
        ] {
            assert!(!codex_notification_for_thread(&params, Some("parent")));
            assert!(codex_notification_for_thread(&params, Some("child")));
        }
    }

    #[test]
    fn unscoped_connection_events_and_initial_handshake_still_pass() {
        assert!(codex_notification_for_thread(
            &json!({"message": "connection event"}),
            Some("parent")
        ));
        assert!(codex_notification_for_thread(
            &json!({"threadId": "parent"}),
            None
        ));
        assert!(codex_notification_for_thread(
            &json!({"item": {"id": "tool"}}),
            Some("parent")
        ));
    }
}

pub(super) fn work_log_from_item(params: &Value, status: WorkLogStatus) -> Option<WorkLogEntry> {
    let item = params.get("item")?;
    let id = item
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("codex-item")
        .to_string();
    let item_type = item.get("type").and_then(Value::as_str).unwrap_or("tool");
    if matches!(item_type, "agentMessage" | "reasoning" | "fileChange") {
        return None;
    }
    let title = item
        .get("title")
        .or_else(|| item.get("name"))
        .or_else(|| item.get("command"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| humanize_item_type(item_type));
    let kind = match item_type {
        "plan" => WorkLogEntryKind::Plan,
        "commandExecution" => WorkLogEntryKind::Command,
        _ => WorkLogEntryKind::Tool,
    };
    Some(WorkLogEntry::new(id.clone(), id, kind, title, status))
}

pub(super) fn plan_text_from_completed_item(params: &Value) -> Option<String> {
    let item = params.get("item")?;
    if item.get("type").and_then(Value::as_str) != Some("plan") {
        return None;
    }
    item.get("text")
        .or_else(|| item.get("summary"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

pub(super) fn generated_image_markdown_from_item(params: &Value) -> Option<String> {
    let item = params.get("item").unwrap_or(params);
    let path = find_generated_image_path(item)?;
    let label = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Generated image");
    Some(format!(
        "Generated image:\n\n![Generated image]({})\n\n`{label}`",
        path.display()
    ))
}

pub(super) fn find_generated_image_path(value: &Value) -> Option<PathBuf> {
    match value {
        Value::Object(map) => {
            for key in ["saved_path", "savedPath", "path", "file", "url"] {
                if let Some(path) = map
                    .get(key)
                    .and_then(Value::as_str)
                    .and_then(parse_generated_image_path)
                {
                    return Some(path);
                }
            }
            map.values().find_map(find_generated_image_path)
        }
        Value::Array(items) => items.iter().find_map(find_generated_image_path),
        Value::String(text) if text.len() <= 4096 => parse_generated_image_path(text),
        _ => None,
    }
}

pub(super) fn parse_generated_image_path(text: &str) -> Option<PathBuf> {
    let trimmed = text.trim().trim_matches(['"', '\'', '`']);
    let candidate = trimmed
        .strip_prefix("file://")
        .unwrap_or(trimmed)
        .split_whitespace()
        .next()
        .unwrap_or(trimmed)
        .trim_end_matches([',', ')', ']']);
    if !candidate.contains("/generated_images/") {
        return None;
    }
    let path = PathBuf::from(candidate);
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)?;
    let is_image = matches!(
        extension.as_str(),
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "tif" | "tiff" | "svg"
    );
    (path.is_absolute() && is_image && path.exists()).then_some(path)
}

pub(super) fn humanize_item_type(item_type: &str) -> String {
    match item_type {
        "plan" => "Plan".to_string(),
        "commandExecution" => "Tool call".to_string(),
        "fileChange" => "File change".to_string(),
        "agentMessage" => "Agent message".to_string(),
        "reasoning" => "Reasoning".to_string(),
        _ => "Tool call".to_string(),
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum CodexErrorDisposition {
    Retry {
        message: String,
        detail: Option<String>,
        turn_id: Option<String>,
    },
    Terminal {
        message: String,
    },
}

pub(super) fn codex_error_disposition(params: &Value) -> CodexErrorDisposition {
    let error = params.get("error").unwrap_or(params);
    let message = error
        .as_str()
        .or_else(|| error.get("message").and_then(Value::as_str))
        .or_else(|| params.get("message").and_then(Value::as_str))
        .map(str::trim)
        .filter(|message| !message.is_empty())
        .unwrap_or("Codex encountered an unexpected error.");
    let detail = error
        .get("additionalDetails")
        .or_else(|| error.get("additional_details"))
        .or_else(|| params.get("additionalDetails"))
        .or_else(|| params.get("additional_details"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|detail| !detail.is_empty());

    if params
        .get("willRetry")
        .or_else(|| params.get("will_retry"))
        .or_else(|| error.get("willRetry"))
        .or_else(|| error.get("will_retry"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return CodexErrorDisposition::Retry {
            message: message.to_string(),
            detail: detail.map(str::to_string),
            turn_id: params
                .get("turnId")
                .or_else(|| params.get("turn_id"))
                .and_then(Value::as_str)
                .map(str::to_string),
        };
    }

    let message = match detail {
        Some(detail) if detail != message => format!("{message}\n\n{detail}"),
        _ => message.to_string(),
    };
    CodexErrorDisposition::Terminal { message }
}

pub(super) fn codex_event_confirms_recovery(method: &str) -> bool {
    method == "turn/completed" || method == "turn/plan/updated" || method.starts_with("item/")
}

pub(super) fn format_plan_summary(params: &Value) -> String {
    let count = params
        .get("plan")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    if count == 1 {
        "Worked on 1 step".to_string()
    } else {
        format!("Worked on {count} steps")
    }
}

pub(super) fn format_plan_detail(params: &Value) -> Option<String> {
    let plan = params.get("plan").and_then(Value::as_array)?;
    let lines = plan
        .iter()
        .filter_map(|entry| {
            let text = entry
                .get("text")
                .or_else(|| entry.get("title"))
                .or_else(|| entry.get("step"))
                .or_else(|| entry.get("description"))
                .and_then(Value::as_str)?
                .trim();
            if text.is_empty() {
                return None;
            }
            let status = entry
                .get("status")
                .or_else(|| entry.get("state"))
                .and_then(Value::as_str)
                .unwrap_or("pending");
            Some(format!("{status}\t{text}"))
        })
        .collect::<Vec<_>>();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

pub(super) fn file_stat_from_patch_change(change: &Value) -> Option<FileChangeStat> {
    let path = change.get("path").and_then(Value::as_str)?;
    let diff = change.get("diff").and_then(Value::as_str).unwrap_or("");
    // Current Codex FileUpdateChange events carry full, unprefixed content for
    // adds/deletes. Only updates (and legacy patch-shaped events) carry hunks.
    // Counting '+'/'-' prefixes in raw content silently reports zero for most
    // new files and miscounts source text that happens to start with a sign.
    let kind = change
        .get("kind")
        .and_then(|kind| kind.get("type"))
        .and_then(Value::as_str);
    let (additions, deletions) = match kind {
        Some("add") => (diff.lines().count(), 0),
        Some("delete") => (0, diff.lines().count()),
        _ => count_unified_diff_lines(diff),
    };
    let mut file = FileChangeStat::new(path, additions, deletions);
    let owned_diff = match kind {
        Some("add") => ide_core::git::diff::diff_from_contents(Path::new(path), "", diff),
        Some("delete") => ide_core::git::diff::diff_from_contents(Path::new(path), diff, ""),
        _ => {
            // Native update events contain only hunks. Use a synthetic header
            // so spaces, tabs, and absolute paths cannot affect patch parsing.
            let patch = if diff.starts_with("@@ ") {
                format!("diff --git a/file b/file\n--- a/file\n+++ b/file\n{diff}")
            } else if diff.starts_with("--- ") {
                let hunks = diff.splitn(3, '\n').nth(2).unwrap_or("");
                format!("diff --git a/file b/file\n--- a/file\n+++ b/file\n{hunks}")
            } else {
                diff.to_string()
            };
            ide_core::git::diff::parse_unified_diff(&patch).map(|diffs| {
                let mut result = ide_core::git::FileDiff::default();
                for diff in diffs {
                    result.hunks.extend(diff.hunks);
                    result.is_binary |= diff.is_binary;
                }
                result
            })
        }
    };
    // Even if the provider patch cannot be rendered, do not substitute a Git
    // diff containing unrelated edits by another conversation on this path.
    let mut owned_diff = owned_diff.unwrap_or_else(|error| {
        eprintln!("failed to parse attributed file-change diff: {error:#}");
        ide_core::git::FileDiff::default()
    });
    owned_diff.path = file.path.clone();
    file.attributed_diff = Some(owned_diff);
    Some(file)
}

pub(super) fn completed_file_change_stats(params: &Value) -> Vec<FileChangeStat> {
    let Some(item) = params.get("item") else {
        return Vec::new();
    };
    if item.get("type").and_then(Value::as_str) != Some("fileChange")
        || item.get("status").and_then(Value::as_str) != Some("completed")
    {
        return Vec::new();
    }
    item.get("changes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(file_stat_from_patch_change)
        .collect()
}

pub(super) fn item_type_from_params(params: &Value) -> Option<&str> {
    params
        .get("item")
        .unwrap_or(params)
        .get("type")
        .and_then(Value::as_str)
}

/// Provider progress events can repeat a file as its patch evolves. Keep the
/// newest provider-owned stat for that path instead of double-counting it.
pub(super) fn upsert_file_change_stats(
    target: &mut Vec<FileChangeStat>,
    incoming: impl IntoIterator<Item = FileChangeStat>,
) {
    for file in incoming {
        if let Some(existing) = target
            .iter_mut()
            .find(|existing| existing.path == file.path)
        {
            *existing = file;
        } else {
            target.push(file);
        }
    }
}

pub(super) fn changed_files_from_unified_diff(diff: &str) -> Vec<FileChangeStat> {
    match ide_core::git::diff::parse_unified_diff(diff) {
        Ok(files) => files
            .into_iter()
            .map(|file| {
                let mut additions = 0;
                let mut deletions = 0;
                for line in file.hunks.iter().flat_map(|hunk| &hunk.lines) {
                    match line.origin {
                        ide_core::git::LineOrigin::Add => additions += 1,
                        ide_core::git::LineOrigin::Remove => deletions += 1,
                        ide_core::git::LineOrigin::Context => {}
                    }
                }
                FileChangeStat::new(file.path, additions, deletions)
            })
            .collect(),
        Err(error) => {
            eprintln!("failed to parse provider file diff: {error:#}");
            Vec::new()
        }
    }
}

pub(super) fn count_unified_diff_lines(diff: &str) -> (usize, usize) {
    let mut additions = 0;
    let mut deletions = 0;
    let mut in_hunk = false;
    for line in diff.lines() {
        if line.starts_with("diff --git ") {
            in_hunk = false;
        } else if line.starts_with("@@ ") {
            in_hunk = true;
        }
        if !in_hunk && (line.starts_with("+++ ") || line.starts_with("--- ")) {
            continue;
        }
        if line.starts_with('+') {
            additions += 1;
        } else if line.starts_with('-') {
            deletions += 1;
        }
    }
    (additions, deletions)
}

pub(super) fn extract_proposed_plan(text: &str) -> Option<String> {
    let start = text.find("<proposed_plan>")? + "<proposed_plan>".len();
    let end = text[start..].find("</proposed_plan>")? + start;
    let plan = text[start..end].trim();
    (!plan.is_empty()).then(|| plan.to_string())
}

pub(super) fn extract_code_review(text: &str) -> Option<String> {
    let start = text.find("<code_review>")? + "<code_review>".len();
    let end = text[start..].find("</code_review>")? + start;
    let review = text[start..end].trim();
    (!review.is_empty()).then(|| review.to_string())
}

pub(super) fn extract_verification(text: &str) -> Option<String> {
    let start = text.find("<verification>")? + "<verification>".len();
    let end = text[start..].find("</verification>")? + start;
    let verification = text[start..end].trim();
    (!verification.is_empty()).then(|| verification.to_string())
}

pub(super) fn extract_review_checklist(text: &str) -> Option<String> {
    let start = text.find("<review_checklist>")? + "<review_checklist>".len();
    let end = text[start..].find("</review_checklist>")? + start;
    Some(text[start..end].trim().to_string())
}

pub(super) fn next_request_id() -> String {
    REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed).to_string()
}

pub(super) const CODEX_DEFAULT_MODE_DEVELOPER_INSTRUCTIONS: &str = r#"<collaboration_mode># Collaboration Mode: Default

You are now in Default mode. Any previous instructions for other modes (e.g. Plan mode) are no longer active.

Your active mode changes only when new developer instructions with a different `<collaboration_mode>...</collaboration_mode>` change it; user requests or tool descriptions do not change mode by themselves. Known mode names are Default and Plan.

## request_user_input availability

The `request_user_input` tool is unavailable in Default mode. If you call it while in Default mode, it will return an error.

In Default mode, strongly prefer making reasonable assumptions and executing the user's request rather than stopping to ask questions.
If you absolutely must ask a question because the answer cannot be discovered from local context and a reasonable assumption would be risky, ask the user directly with a concise plain-text question. Never write a multiple-choice question as a textual assistant message.
</collaboration_mode>"#;

pub(super) const CODEX_PLAN_MODE_DEVELOPER_INSTRUCTIONS: &str = r#"<collaboration_mode># Plan Mode (Conversational)

You work in 3 phases, and you should chat your way to a great plan before finalizing it. A great plan is detailed enough in intent and implementation that it can be handed to another engineer or agent to implement immediately. It must be decision complete, where the implementer does not need to make product or technical decisions.

## Mode rules (strict)

You are in Plan Mode until a developer message explicitly ends it.

Plan Mode is not changed by user intent, tone, or imperative language. If a user asks for execution while still in Plan Mode, treat it as a request to plan the execution, not perform it.

## Plan Mode vs update_plan tool

Plan Mode is a collaboration mode that can involve requesting user input and eventually issuing a `<proposed_plan>` block.

Separately, `update_plan` is a checklist/progress/TODOs tool; it does not enter or exit Plan Mode. Do not confuse it with Plan Mode or use it while in Plan Mode.

## Execution vs mutation in Plan Mode

You may explore and execute non-mutating actions that improve the plan. You must not perform mutating actions.

Allowed non-mutating actions:

* Reading or searching files, configs, schemas, types, manifests, and docs
* Static analysis, inspection, and repo exploration
* Dry-run style commands when they do not edit repo-tracked files
* Tests, builds, or checks that may write to caches or build artifacts, so long as they do not edit repo-tracked files

Not allowed mutating actions:

* Editing or writing files
* Running formatters or linters that rewrite files
* Applying patches, migrations, or codegen that updates repo-tracked files
* Side-effectful commands whose purpose is to carry out the plan rather than refine it

When in doubt: if the action would reasonably be described as doing the work rather than planning the work, do not do it.

## Phase 1 - Ground in the environment

Begin by grounding yourself in the actual environment. Eliminate unknowns in the prompt by discovering facts, not by asking the user. Resolve all questions that can be answered through exploration or inspection. Identify missing or ambiguous details only if they cannot be derived from the environment. Silent exploration between turns is allowed and encouraged.

Before asking the user any question, perform at least one targeted non-mutating exploration pass, unless no local environment or repo is available.

Exception: you may ask clarifying questions about the user's prompt before exploring only if there are obvious ambiguities or contradictions in the prompt itself. If ambiguity might be resolved by exploring, prefer exploring first.

Do not ask questions that can be answered from the repo or system. Ask only once you have exhausted reasonable non-mutating exploration.

## Phase 2 - Intent chat

Keep asking until you can clearly state: goal and success criteria, audience, in/out of scope, constraints, current state, and key preferences/tradeoffs.

Bias toward questions over guessing: if any high-impact ambiguity remains, do not plan yet. Ask.

## Phase 3 - Implementation chat

Once intent is stable, keep asking until the spec is decision complete: approach, interfaces (APIs/schemas/I/O), data flow, edge cases/failure modes, testing and acceptance criteria, rollout/monitoring, and migration/compatibility constraints.

## Asking questions

Critical rules:

* Strongly prefer using the `request_user_input` tool to ask any questions.
* Offer only meaningful multiple-choice options; do not include filler choices that are obviously wrong or irrelevant.
* In rare cases where an unavoidable important question cannot be expressed with reasonable multiple-choice options, ask it directly without the tool.

You should ask many questions, but each question must:

* materially change the spec or plan, or
* confirm/lock an assumption, or
* choose between meaningful tradeoffs, and
* not be answerable by non-mutating commands.

Use the `request_user_input` tool only for decisions that materially change the plan, for confirming important assumptions, or for information that cannot be discovered via non-mutating exploration.

## Two kinds of unknowns

1. Discoverable facts (repo/system truth): explore first.

   * Before asking, run targeted searches and check likely sources of truth.
   * Ask only if multiple plausible candidates remain, nothing found but a missing identifier/context is required, or ambiguity is product intent.
   * If asking, present concrete candidates and recommend one.
   * Never ask questions you can answer from the environment.

2. Preferences/tradeoffs (not discoverable): ask early.

   * These are intent or implementation preferences that cannot be derived from exploration.
   * Provide 2-4 mutually exclusive options and a recommended default.
   * If unanswered, proceed with the recommended option and record it as an assumption in the final plan.

## Finalization rule

When the plan is complete, wrap it exactly in:

<proposed_plan>
plan content
</proposed_plan>

The opening tag must be on its own line. Start the plan content on the next line. The closing tag must be on its own line. Use Markdown inside the block. Keep the tags exactly as `<proposed_plan>` and `</proposed_plan>`.

The final plan must be plan-only and include:

* A clear title
* A brief summary section
* Important changes or additions to public APIs/interfaces/types
* Test cases and scenarios
* Explicit assumptions and defaults chosen where needed

Do not ask "should I proceed?" in the final output. The user can switch out of Plan Mode and request implementation if you included a `<proposed_plan>` block.

Only produce at most one `<proposed_plan>` block per turn, and only when presenting a complete spec.

Do not implement while in Plan Mode.
</collaboration_mode>"#;

#[cfg(test)]
mod work_log_tests {
    use super::*;

    #[test]
    fn codex_raw_file_content_has_correct_counts_for_creations_and_deletions() {
        // The first case is the actual event shape from the desktop demo test.
        for (kind, content, expected) in [
            ("add", "beta-owned-by-B\n", (1, 0)),
            ("add", "alpha-owned-by-A\nsecond-line-A\n", (2, 0)),
            ("add", "+source text\n-source text\n@@ not a hunk", (3, 0)),
            ("delete", "first\nsecond\n", (0, 2)),
            ("add", "", (0, 0)),
        ] {
            let params = json!({"item": {
                "type": "fileChange", "status": "completed",
                "changes": [{"path": "qa.txt", "kind": {"type": kind}, "diff": content}]
            }});
            let files = completed_file_change_stats(&params);
            assert_eq!(files.len(), 1);
            assert_eq!((files[0].additions, files[0].deletions), expected);
            let snapshot = files[0].attributed_diff.as_ref().unwrap();
            let lines = snapshot.hunks.iter().flat_map(|hunk| &hunk.lines);
            assert_eq!(
                lines
                    .filter(|line| line.origin != ide_core::git::LineOrigin::Context)
                    .count(),
                expected.0 + expected.1
            );
        }
        let update = json!({"path": "qa.txt", "kind": {"type": "update", "move_path": null},
            "diff": "@@ -1 +1,2 @@\n beta-owned-by-B\n+beta-second-line-B\n"});
        let file = file_stat_from_patch_change(&update).unwrap();
        assert_eq!((file.additions, file.deletions), (1, 0));
    }

    #[test]
    fn codex_shared_file_snapshot_highlights_only_the_completed_edit() {
        use ide_core::git::LineOrigin;
        let change = json!({"path": "/project/space and\ttab.txt",
            "kind": {"type": "update", "move_path": null},
            "diff": "@@ -1,2 +1,3 @@\n alpha-owned-by-A\n second-line-A\n+third-line-written-by-B\n"});
        let file = file_stat_from_patch_change(&change).unwrap();
        let diff = file.attributed_diff.as_ref().unwrap();
        assert_eq!(diff.path, file.path);
        assert_eq!((file.additions, file.deletions), (1, 0));
        assert_eq!(diff.hunks.len(), 1);
        assert_eq!(
            diff.hunks[0]
                .lines
                .iter()
                .map(|line| line.origin)
                .collect::<Vec<_>>(),
            vec![LineOrigin::Context, LineOrigin::Context, LineOrigin::Add]
        );
        assert_eq!(diff.hunks[0].lines[2].text, "third-line-written-by-B");
    }

    #[test]
    fn malformed_owned_patch_does_not_allow_shared_worktree_substitution() {
        let file = file_stat_from_patch_change(&json!({
            "path": "shared.txt", "kind": {"type": "update"}, "diff": "not a patch"
        }))
        .unwrap();
        assert!(file.attributed_diff.is_some());
        assert!(file.attributed_diff.unwrap().hunks.is_empty());
    }

    #[test]
    fn diff_receipt_preserves_deletions_renames_binary_and_quoted_paths() {
        let patch = concat!(
            "diff --git a/first.txt b/first.txt\n--- a/first.txt\n+++ b/first.txt\n@@ -1 +1 @@\n-old\n+new\n",
            "diff --git a/deleted.txt b/deleted.txt\ndeleted file mode 100644\n--- a/deleted.txt\n+++ /dev/null\n@@ -1 +0,0 @@\n-gone\n",
            "diff --git a/old.txt b/renamed.txt\nsimilarity index 100%\nrename from old.txt\nrename to renamed.txt\n",
            "diff --git a/image.bin b/image.bin\nindex 1234567..abcdef0 100644\nBinary files a/image.bin and b/image.bin differ\n",
            "diff --git \"a/tab\\tname.txt\" \"b/tab\\tname.txt\"\n--- \"a/tab\\tname.txt\"\n+++ \"b/tab\\tname.txt\"\n@@ -1 +1 @@\n--- old content\n+++ new content\n",
        );
        assert_eq!(
            changed_files_from_unified_diff(patch),
            vec![
                FileChangeStat::new("first.txt", 1, 1),
                FileChangeStat::new("deleted.txt", 0, 1),
                FileChangeStat::new("renamed.txt", 0, 0),
                FileChangeStat::new("image.bin", 0, 0),
                FileChangeStat::new("tab\tname.txt", 1, 1),
            ]
        );
        assert_eq!(count_unified_diff_lines(patch), (2, 3));
    }

    #[test]
    fn codex_compaction_uses_item_lifecycle_and_legacy_completion() {
        let params = json!({"item": {"id": "compact-1", "type": "contextCompaction"}});
        assert_eq!(
            codex_compaction_activity("item/started", &params),
            Some(true)
        );
        assert_eq!(
            codex_compaction_activity("item/completed", &params),
            Some(false)
        );
        assert_eq!(
            codex_compaction_activity("thread/compacted", &json!({})),
            Some(false)
        );
        assert_eq!(codex_compaction_activity("unrelated", &params), None);
        for item_type in ["commandExecution", "reasoning", "agentMessage"] {
            let params = json!({"item": {"type": item_type}});
            assert_eq!(codex_compaction_activity("item/started", &params), None);
            assert_eq!(codex_compaction_activity("item/completed", &params), None);
        }
        assert_eq!(codex_compaction_activity("item/started", &json!({})), None);
    }

    #[test]
    fn completed_file_change_item_reports_a_new_file_without_streaming_events() {
        let params = json!({
            "item": {
                "id": "file-change-1",
                "type": "fileChange",
                "status": "completed",
                "changes": [{
                    "path": "simple-mock.html",
                    "kind": "add",
                    "diff": "--- /dev/null\n+++ b/simple-mock.html\n@@ -0,0 +1,2 @@\n+<!doctype html>\n+<title>Mock</title>\n"
                }]
            }
        });

        let files = completed_file_change_stats(&params);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, Path::new("simple-mock.html"));
        assert_eq!((files[0].additions, files[0].deletions), (2, 0));
        assert_eq!(
            files[0].attributed_diff.as_ref().unwrap().hunks[0]
                .lines
                .len(),
            2
        );
    }

    #[test]
    fn unsuccessful_file_change_item_does_not_report_edits() {
        for status in ["failed", "declined"] {
            let params = json!({
                "item": {
                    "id": "file-change-1",
                    "type": "fileChange",
                    "status": status,
                    "changes": [{"path": "not-created.html", "kind": "add", "diff": "+nope"}]
                }
            });

            assert!(completed_file_change_stats(&params).is_empty());
        }
    }

    #[test]
    fn reasoning_and_duplicate_file_change_items_do_not_become_work_logs() {
        for item_type in ["reasoning", "fileChange", "agentMessage"] {
            let params = json!({ "item": { "id": "item-1", "type": item_type } });
            assert!(work_log_from_item(&params, WorkLogStatus::Completed).is_none());
        }
    }

    #[test]
    fn command_items_remain_visible_actions() {
        let params = json!({
            "item": {
                "id": "command-1",
                "type": "commandExecution",
                "command": "cargo test -p ide-app"
            }
        });

        let entry = work_log_from_item(&params, WorkLogStatus::Completed)
            .expect("commands should remain in the activity timeline");

        assert_eq!(entry.kind, WorkLogEntryKind::Command);
        assert_eq!(entry.title, "cargo test -p ide-app");
    }

    #[test]
    fn bridge_commands_keep_their_command_kind() {
        let event = json!({
            "id": "bash-1",
            "type": "work_log",
            "kind": "command",
            "title": "Bash",
            "status": "completed",
            "detail": "npm test"
        });

        let entry = work_log_from_bridge_event(&event).expect("valid bridge work log");

        assert_eq!(entry.kind, WorkLogEntryKind::Command);
    }
}

#[cfg(test)]
mod approval_tests {
    use super::*;

    #[test]
    fn repeated_patch_progress_replaces_only_the_matching_path() {
        let mut target = vec![FileChangeStat::new("src/a.rs", 1, 0)];
        upsert_file_change_stats(
            &mut target,
            [
                FileChangeStat::new("src/a.rs", 4, 2),
                FileChangeStat::new("src/b.rs", 3, 0),
            ],
        );
        assert_eq!(
            target,
            vec![
                FileChangeStat::new("src/a.rs", 4, 2),
                FileChangeStat::new("src/b.rs", 3, 0),
            ]
        );
    }

    #[test]
    fn command_approval_keeps_command_and_reason() {
        let pending = pending_approval_from_codex_request(
            "request-1".into(),
            "item/commandExecution/requestApproval",
            &json!({
                "command": "npm publish",
                "reason": "Publish the package",
                "cwd": "/workspace"
            }),
        );
        assert_eq!(pending.kind, PendingApprovalKind::Command);
        assert_eq!(pending.request_id, "request-1");
        let detail = pending.detail.unwrap();
        assert!(detail.contains("npm publish"));
        assert!(detail.contains("Publish the package"));
    }

    #[test]
    fn permission_approval_summarizes_network_and_files() {
        let pending = pending_approval_from_codex_request(
            "request-2".into(),
            "item/permissions/requestApproval",
            &json!({
                "permissions": {
                    "network": { "enabled": true },
                    "fileSystem": {
                        "write": ["/tmp/output"],
                        "read": ["/tmp/input"]
                    }
                }
            }),
        );
        assert_eq!(pending.kind, PendingApprovalKind::Permissions);
        let detail = pending.detail.unwrap();
        assert!(detail.contains("Network access"));
        assert!(detail.contains("2 paths"));
    }
}

#[cfg(test)]
mod codex_error_tests {
    use super::*;

    #[test]
    fn retryable_disconnect_is_not_a_terminal_error() {
        let disposition = codex_error_disposition(&json!({
            "error": {
                "message": "Reconnecting... 2/5",
                "additionalDetails": "websocket closed before response.completed"
            },
            "willRetry": true,
            "turnId": "turn-123"
        }));

        assert_eq!(
            disposition,
            CodexErrorDisposition::Retry {
                message: "Reconnecting... 2/5".into(),
                detail: Some("websocket closed before response.completed".into()),
                turn_id: Some("turn-123".into()),
            }
        );
    }

    #[test]
    fn exhausted_retry_uses_a_readable_terminal_error() {
        let disposition = codex_error_disposition(&json!({
            "error": {
                "message": "Unable to reconnect",
                "additionalDetails": "websocket remained unavailable"
            },
            "willRetry": false
        }));

        assert_eq!(
            disposition,
            CodexErrorDisposition::Terminal {
                message: "Unable to reconnect\n\nwebsocket remained unavailable".into(),
            }
        );
    }

    #[test]
    fn string_error_payload_remains_readable() {
        assert_eq!(
            codex_error_disposition(&json!({ "error": "connection refused" })),
            CodexErrorDisposition::Terminal {
                message: "connection refused".into(),
            }
        );
    }

    #[test]
    fn item_and_turn_events_confirm_recovery() {
        assert!(codex_event_confirms_recovery("item/agentMessage/delta"));
        assert!(codex_event_confirms_recovery("item/tool/requestUserInput"));
        assert!(codex_event_confirms_recovery("turn/completed"));
        assert!(!codex_event_confirms_recovery("thread/tokenUsage/updated"));
        assert!(!codex_event_confirms_recovery("error"));
    }
}
