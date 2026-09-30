use super::*;

pub(super) fn linear_state_category(state_type: String) -> String {
    match state_type.as_str() {
        "completed" | "canceled" => "done",
        "started" => "in progress",
        // backlog, unstarted, triage, and anything new
        _ => "new",
    }
    .to_string()
}

pub(super) fn ensure_linear_success(value: &Value, pointer: &str, action: &str) -> Result<()> {
    if value.pointer(pointer).and_then(Value::as_bool) == Some(true) {
        return Ok(());
    }
    let detail = value
        .get("errors")
        .and_then(Value::as_array)
        .and_then(|errors| errors.first())
        .and_then(|error| string_field(error, "message"))
        .unwrap_or_else(|| "unknown error".to_string());
    Err(anyhow!("failed to {action}: {detail}"))
}

pub(super) fn response_json(
    response: reqwest::blocking::Response,
    provider: &str,
    connection: &TaskTrackerConnection,
) -> Result<Value> {
    let status = response.status();
    let text = response
        .text()
        .with_context(|| format!("failed to read {provider} response"))?;
    if !status.is_success() {
        let text = connection.redact_diagnostic(&text);
        return Err(anyhow!("{provider} request failed with {status}: {text}"));
    }
    serde_json::from_str(&text).with_context(|| format!("failed to parse {provider} response"))
}

pub(super) fn linear_api_url(connection: &TaskTrackerConnection) -> String {
    let configured = normalize_site_url(&connection.site_url);
    if configured.is_empty() {
        "https://api.linear.app/graphql".to_string()
    } else if configured.ends_with("/graphql") {
        configured
    } else {
        format!("{configured}/graphql")
    }
}

pub(super) fn clickup_api_base(connection: &TaskTrackerConnection) -> String {
    let configured = normalize_site_url(&connection.site_url);
    if configured.is_empty() {
        "https://api.clickup.com".to_string()
    } else {
        configured
    }
}

pub(super) fn asana_api_base(connection: &TaskTrackerConnection) -> String {
    let configured = normalize_site_url(&connection.site_url);
    if configured.is_empty() {
        "https://app.asana.com".to_string()
    } else {
        configured
    }
}

pub(super) fn ensure_columns(
    mut columns: Vec<TaskBoardColumn>,
    issues: &[TaskSummary],
) -> Vec<TaskBoardColumn> {
    for issue in issues {
        if !columns.iter().any(|column| column.name == issue.column) {
            columns.push(TaskBoardColumn {
                name: issue.column.clone(),
                status_ids: vec![issue.status_id.clone()],
            });
        }
    }
    if columns.is_empty() {
        columns.push(TaskBoardColumn {
            name: "Tasks".to_string(),
            status_ids: Vec::new(),
        });
    }
    columns
}

pub(super) fn linear_issue_summary(issue: &Value) -> Result<TaskSummary> {
    let issue_id = string_field(issue, "id").context("Linear issue missing id")?;
    let issue_key = string_field(issue, "identifier").unwrap_or_else(|| issue_id.clone());
    let title = string_field(issue, "title").unwrap_or_else(|| issue_key.clone());
    let state = issue.get("state").unwrap_or(&Value::Null);
    let status_id = string_field(state, "id").unwrap_or_default();
    let status = string_field(state, "name").unwrap_or_else(|| "No status".to_string());
    let status_category = string_field(state, "type");
    let issue_url = string_field(issue, "url").unwrap_or_default();
    let site_url = issue_url
        .split("/issue/")
        .next()
        .filter(|url| !url.is_empty())
        .unwrap_or("https://linear.app")
        .to_string();
    Ok(TaskSummary {
        reference: TaskRef {
            provider: IssueTrackerProvider::Linear,
            site_url,
            issue_id,
            issue_key,
            issue_url,
            title,
        },
        status_id,
        status: status.clone(),
        status_category,
        column: status,
        assignee: issue
            .get("assignee")
            .and_then(|assignee| string_field(assignee, "name")),
        priority: string_field(issue, "priorityLabel"),
        issue_type: Some("Issue".to_string()),
        labels: issue
            .pointer("/labels/nodes")
            .and_then(Value::as_array)
            .map(|labels| {
                labels
                    .iter()
                    .filter_map(|label| string_field(label, "name"))
                    .collect()
            })
            .unwrap_or_default(),
        updated: string_field(issue, "updatedAt"),
        created: string_field(issue, "createdAt"),
    })
}

pub(super) fn clickup_task_summary(
    task: &Value,
    connection: &TaskTrackerConnection,
) -> Result<TaskSummary> {
    let issue_id = string_field(task, "id").context("ClickUp task missing id")?;
    let title = string_field(task, "name").unwrap_or_else(|| issue_id.clone());
    let status = task.get("status").unwrap_or(&Value::Null);
    let status_name = string_field(status, "status").unwrap_or_else(|| "No status".to_string());
    let issue_url = string_field(task, "url").unwrap_or_else(|| {
        format!(
            "{}/t/{}",
            normalize_site_url(&connection.site_url)
                .trim_end_matches("/api")
                .trim_end_matches("/api/v2"),
            issue_id
        )
    });
    let assignee = task
        .get("assignees")
        .and_then(Value::as_array)
        .and_then(|assignees| assignees.first())
        .and_then(|assignee| {
            string_field(assignee, "username").or_else(|| string_field(assignee, "email"))
        });
    Ok(TaskSummary {
        reference: TaskRef {
            provider: IssueTrackerProvider::ClickUp,
            site_url: normalize_site_url(&connection.site_url),
            issue_id: issue_id.clone(),
            issue_key: issue_id,
            issue_url,
            title,
        },
        status_id: status_name.clone(),
        status: status_name.clone(),
        status_category: status
            .get("type")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        column: status_name,
        assignee,
        priority: task
            .get("priority")
            .and_then(|priority| string_field(priority, "priority")),
        issue_type: Some("Task".to_string()),
        labels: task
            .get("tags")
            .and_then(Value::as_array)
            .map(|tags| {
                tags.iter()
                    .filter_map(|tag| string_field(tag, "name"))
                    .collect()
            })
            .unwrap_or_default(),
        updated: string_field(task, "date_updated"),
        created: string_field(task, "date_created"),
    })
}

pub(super) fn clickup_attachments(task: &Value) -> Vec<TaskAttachment> {
    task.get("attachments")
        .and_then(Value::as_array)
        .map(|attachments| {
            attachments
                .iter()
                .filter_map(|attachment| {
                    let id = string_field(attachment, "id")
                        .or_else(|| string_field(attachment, "version"))
                        .unwrap_or_else(|| Uuid::new_v4().to_string());
                    let filename = string_field(attachment, "title")
                        .or_else(|| string_field(attachment, "filename"))
                        .unwrap_or_else(|| format!("attachment-{id}"));
                    Some(TaskAttachment {
                        id,
                        filename,
                        mime_type: string_field(attachment, "mimetype"),
                        content_url: string_field(attachment, "url"),
                        thumbnail_url: string_field(attachment, "thumbnail_small")
                            .or_else(|| string_field(attachment, "thumbnail_medium")),
                        local_path: None,
                        size: attachment.get("size").and_then(Value::as_u64).or_else(|| {
                            attachment.get("size").and_then(Value::as_str)?.parse().ok()
                        }),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn asana_task_summary(
    task: &Value,
    connection: &TaskTrackerConnection,
) -> Result<TaskSummary> {
    let issue_id = string_field(task, "gid").context("Asana task missing gid")?;
    let title = string_field(task, "name").unwrap_or_else(|| issue_id.clone());
    let completed = task
        .get("completed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let section = task
        .get("memberships")
        .and_then(Value::as_array)
        .and_then(|memberships| memberships.first())
        .and_then(|membership| membership.get("section"))
        .and_then(|section| string_field(section, "name"));
    let status = if completed {
        "Done".to_string()
    } else {
        section.unwrap_or_else(|| "Open".to_string())
    };
    Ok(TaskSummary {
        reference: TaskRef {
            provider: IssueTrackerProvider::Asana,
            site_url: normalize_site_url(&connection.site_url),
            issue_id: issue_id.clone(),
            issue_key: issue_id,
            issue_url: string_field(task, "permalink_url").unwrap_or_default(),
            title,
        },
        status_id: status.clone(),
        status: status.clone(),
        status_category: completed.then(|| "done".to_string()),
        column: status,
        assignee: task
            .get("assignee")
            .and_then(|assignee| string_field(assignee, "name")),
        priority: None,
        issue_type: Some("Task".to_string()),
        labels: task
            .get("tags")
            .and_then(Value::as_array)
            .map(|tags| {
                tags.iter()
                    .filter_map(|tag| string_field(tag, "name"))
                    .collect()
            })
            .unwrap_or_default(),
        updated: string_field(task, "modified_at"),
        created: string_field(task, "created_at"),
    })
}

pub fn normalize_site_url(value: &str) -> String {
    value.trim().trim_end_matches('/').to_string()
}

pub fn jira_assignee_filter_to_jql(value: &str) -> Option<String> {
    let filter = value.trim();
    if filter.is_empty() {
        return None;
    }

    let lower = filter.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "me" | "mine" | "currentuser" | "currentuser()"
    ) {
        return Some("assignee = currentUser()".to_string());
    }
    if matches!(lower.as_str(), "unassigned" | "none") {
        return Some("assignee is EMPTY".to_string());
    }
    if lower.starts_with("assignee ") || lower.starts_with("assignee=") {
        return Some(filter.to_string());
    }

    Some(format!("assignee = {}", jira_jql_operand(filter)))
}

pub(super) fn jira_jql_operand(value: &str) -> String {
    if value.ends_with("()")
        || value.eq_ignore_ascii_case("empty")
        || (value.starts_with('"') && value.ends_with('"'))
    {
        return value.to_string();
    }
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

pub fn jira_adf_to_text(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.to_string();
    }
    let mut out = String::new();
    adf_node_to_text(value, 0, &mut out);
    compact_text_lines(&out)
}

fn adf_node_to_text(value: &Value, depth: usize, out: &mut String) {
    let node_type = value.get("type").and_then(Value::as_str).unwrap_or("");
    match node_type {
        "doc" => adf_children_to_text(value, depth, out, "\n\n"),
        "paragraph" => {
            adf_children_to_text(value, depth, out, "");
            out.push('\n');
        }
        "heading" => {
            let level = value
                .get("attrs")
                .and_then(|attrs| attrs.get("level"))
                .and_then(Value::as_u64)
                .unwrap_or(2);
            out.push_str(&"#".repeat(level as usize));
            out.push(' ');
            adf_children_to_text(value, depth, out, "");
            out.push('\n');
        }
        "bulletList" => adf_children_to_text(value, depth + 1, out, ""),
        "orderedList" => adf_children_to_text(value, depth + 1, out, ""),
        "listItem" => {
            out.push_str(&"  ".repeat(depth.saturating_sub(1)));
            out.push_str("- ");
            adf_children_to_text(value, depth, out, "");
            if !out.ends_with('\n') {
                out.push('\n');
            }
        }
        "blockquote" => {
            let mut inner = String::new();
            adf_children_to_text(value, depth, &mut inner, "");
            for line in compact_text_lines(&inner).lines() {
                out.push_str("> ");
                out.push_str(line);
                out.push('\n');
            }
        }
        "codeBlock" => {
            out.push_str("```\n");
            adf_children_to_text(value, depth, out, "");
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("```\n");
        }
        "text" => {
            if let Some(text) = value.get("text").and_then(Value::as_str) {
                out.push_str(text);
            }
        }
        "hardBreak" => out.push('\n'),
        "emoji" => {
            if let Some(short_name) = value
                .get("attrs")
                .and_then(|attrs| attrs.get("shortName"))
                .and_then(Value::as_str)
            {
                out.push_str(short_name);
            }
        }
        "mention" => {
            if let Some(text) = value
                .get("attrs")
                .and_then(|attrs| attrs.get("text"))
                .and_then(Value::as_str)
            {
                out.push_str(text);
            }
        }
        "media" => {
            out.push_str(&media_placeholder_text(value));
            out.push('\n');
        }
        _ => adf_children_to_text(value, depth, out, ""),
    }
}

pub fn jira_adf_to_rich_text(value: &Value, attachments: &[TaskAttachment]) -> TaskRichText {
    if let Some(text) = value.as_str() {
        return TaskRichText::plain(text);
    }
    let text = jira_adf_to_text(value);
    let mut builder = RichTextBuilder::default();
    adf_node_to_rich_blocks(value, 0, attachments, &mut builder);
    TaskRichText {
        blocks: builder.finish(&text),
        text,
    }
}

#[derive(Default)]
struct RichTextBuilder {
    pending_text: String,
    blocks: Vec<TaskContentBlock>,
}

impl RichTextBuilder {
    fn push_text(&mut self, text: &str) {
        self.pending_text.push_str(text);
    }

    fn push_image(&mut self, image: TaskInlineImage) {
        self.flush_text();
        self.blocks.push(TaskContentBlock::Image(image));
    }

    fn flush_text(&mut self) {
        let text = compact_text_lines(&self.pending_text);
        self.pending_text.clear();
        if !text.is_empty() {
            self.blocks.push(TaskContentBlock::Text(text));
        }
    }

    fn finish(mut self, fallback_text: &str) -> Vec<TaskContentBlock> {
        self.flush_text();
        if self.blocks.is_empty() && !fallback_text.trim().is_empty() {
            self.blocks
                .push(TaskContentBlock::Text(fallback_text.to_string()));
        }
        self.blocks
    }
}

fn adf_node_to_rich_blocks(
    value: &Value,
    depth: usize,
    attachments: &[TaskAttachment],
    builder: &mut RichTextBuilder,
) {
    let node_type = value.get("type").and_then(Value::as_str).unwrap_or("");
    match node_type {
        "doc" => adf_children_to_rich_blocks(value, depth, attachments, builder, "\n\n"),
        "paragraph" => {
            adf_children_to_rich_blocks(value, depth, attachments, builder, "");
            builder.push_text("\n");
        }
        "heading" => {
            let level = value
                .get("attrs")
                .and_then(|attrs| attrs.get("level"))
                .and_then(Value::as_u64)
                .unwrap_or(2);
            builder.push_text(&"#".repeat(level as usize));
            builder.push_text(" ");
            adf_children_to_rich_blocks(value, depth, attachments, builder, "");
            builder.push_text("\n");
        }
        "bulletList" => adf_children_to_rich_blocks(value, depth + 1, attachments, builder, ""),
        "orderedList" => adf_children_to_rich_blocks(value, depth + 1, attachments, builder, ""),
        "listItem" => {
            builder.push_text(&"  ".repeat(depth.saturating_sub(1)));
            builder.push_text("- ");
            adf_children_to_rich_blocks(value, depth, attachments, builder, "");
            builder.push_text("\n");
        }
        "blockquote" => {
            builder.push_text(&jira_adf_to_text(value));
            builder.push_text("\n");
        }
        "codeBlock" => {
            builder.push_text("```\n");
            adf_children_to_rich_blocks(value, depth, attachments, builder, "");
            builder.push_text("\n```\n");
        }
        "text" => {
            if let Some(text) = value.get("text").and_then(Value::as_str) {
                builder.push_text(text);
            }
        }
        "hardBreak" => builder.push_text("\n"),
        "emoji" => {
            if let Some(short_name) = value
                .get("attrs")
                .and_then(|attrs| attrs.get("shortName"))
                .and_then(Value::as_str)
            {
                builder.push_text(short_name);
            }
        }
        "mention" => {
            if let Some(text) = value
                .get("attrs")
                .and_then(|attrs| attrs.get("text"))
                .and_then(Value::as_str)
            {
                builder.push_text(text);
            }
        }
        "media" => builder.push_image(media_node_to_inline_image(value, attachments)),
        _ => adf_children_to_rich_blocks(value, depth, attachments, builder, ""),
    }
}

fn adf_children_to_rich_blocks(
    value: &Value,
    depth: usize,
    attachments: &[TaskAttachment],
    builder: &mut RichTextBuilder,
    separator: &str,
) {
    let Some(children) = value.get("content").and_then(Value::as_array) else {
        return;
    };
    for (index, child) in children.iter().enumerate() {
        if index > 0 && !separator.is_empty() {
            builder.push_text(separator);
        }
        adf_node_to_rich_blocks(child, depth, attachments, builder);
    }
}

fn media_node_to_inline_image(value: &Value, attachments: &[TaskAttachment]) -> TaskInlineImage {
    let attrs = value.get("attrs").unwrap_or(&Value::Null);
    let media_id = string_field(attrs, "id");
    let alt = string_field(attrs, "alt")
        .or_else(|| string_field(attrs, "__fileName"))
        .or_else(|| string_field(attrs, "fileName"));
    let attachment = match_image_attachment(media_id.as_deref(), alt.as_deref(), attachments);
    TaskInlineImage {
        attachment_id: attachment.map(|attachment| attachment.id.clone()),
        media_id,
        filename: attachment
            .map(|attachment| attachment.filename.clone())
            .or_else(|| alt.clone()),
        alt,
        mime_type: attachment.and_then(|attachment| attachment.mime_type.clone()),
        local_path: attachment.and_then(|attachment| attachment.local_path.clone()),
        content_url: attachment.and_then(|attachment| attachment.content_url.clone()),
    }
}

fn match_image_attachment<'a>(
    media_id: Option<&str>,
    alt: Option<&str>,
    attachments: &'a [TaskAttachment],
) -> Option<&'a TaskAttachment> {
    let image_attachments = attachments
        .iter()
        .filter(|attachment| attachment.is_image())
        .collect::<Vec<_>>();
    if let Some(media_id) = media_id.map(str::trim).filter(|value| !value.is_empty()) {
        if let Some(attachment) = image_attachments
            .iter()
            .copied()
            .find(|attachment| attachment.id == media_id)
        {
            return Some(attachment);
        }
    }
    if let Some(alt) = alt.map(str::trim).filter(|value| !value.is_empty()) {
        if let Some(attachment) = image_attachments.iter().copied().find(|attachment| {
            attachment.filename.eq_ignore_ascii_case(alt)
                || Path::new(&attachment.filename)
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .is_some_and(|stem| stem.eq_ignore_ascii_case(alt))
        }) {
            return Some(attachment);
        }
    }
    if image_attachments.len() == 1 {
        return image_attachments.first().copied();
    }
    None
}

fn media_placeholder_text(value: &Value) -> String {
    let attrs = value.get("attrs").unwrap_or(&Value::Null);
    let label = string_field(attrs, "alt")
        .or_else(|| string_field(attrs, "__fileName"))
        .or_else(|| string_field(attrs, "fileName"))
        .or_else(|| string_field(attrs, "id"))
        .unwrap_or_else(|| "image".to_string());
    format!("[image: {label}]")
}

fn adf_children_to_text(value: &Value, depth: usize, out: &mut String, separator: &str) {
    let Some(children) = value.get("content").and_then(Value::as_array) else {
        return;
    };
    for (index, child) in children.iter().enumerate() {
        if index > 0 && !separator.is_empty() && !out.ends_with(separator) {
            out.push_str(separator);
        }
        adf_node_to_text(child, depth, out);
    }
}

fn compact_text_lines(value: &str) -> String {
    let mut lines = Vec::new();
    let mut previous_blank = false;
    for line in value.lines() {
        let trimmed = line.trim_end();
        let blank = trimmed.trim().is_empty();
        if blank && previous_blank {
            continue;
        }
        lines.push(trimmed.to_string());
        previous_blank = blank;
    }
    lines.join("\n").trim().to_string()
}

pub(super) fn column_for_status(columns: &[TaskBoardColumn], status_id: &str) -> Option<String> {
    columns
        .iter()
        .find(|column| column.status_ids.iter().any(|id| id == status_id))
        .map(|column| column.name.clone())
}

pub(super) fn string_field(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

pub(super) fn string_array_field(value: &Value, field: &str) -> Vec<String> {
    value
        .get(field)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn jira_user_from_value(value: &Value) -> Option<JiraUser> {
    let account_id = string_field(value, "accountId")?;
    let display_name = string_field(value, "displayName").unwrap_or_else(|| account_id.clone());
    Some(JiraUser {
        account_id,
        display_name,
        email: string_field(value, "emailAddress"),
        avatar_url: value
            .get("avatarUrls")
            .and_then(|avatars| string_field(avatars, "24x24"))
            .or_else(|| {
                value
                    .get("avatarUrls")
                    .and_then(|avatars| string_field(avatars, "48x48"))
            }),
        active: value.get("active").and_then(Value::as_bool).unwrap_or(true),
    })
}

pub(super) fn parse_task_attachments(fields: &Value) -> Vec<TaskAttachment> {
    fields
        .get("attachment")
        .and_then(Value::as_array)
        .map(|attachments| {
            attachments
                .iter()
                .filter_map(|attachment| {
                    let id = string_field(attachment, "id")?;
                    Some(TaskAttachment {
                        filename: string_field(attachment, "filename")
                            .unwrap_or_else(|| format!("attachment-{id}")),
                        mime_type: string_field(attachment, "mimeType"),
                        content_url: string_field(attachment, "content"),
                        thumbnail_url: string_field(attachment, "thumbnail"),
                        size: attachment.get("size").and_then(Value::as_u64),
                        local_path: None,
                        id,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn image_extension(path: &Path) -> Option<&'static str> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "apng" => Some("apng"),
        "avif" => Some("avif"),
        "gif" => Some("gif"),
        "jpg" | "jpeg" => Some("jpg"),
        "png" => Some("png"),
        "svg" => Some("svg"),
        "webp" => Some("webp"),
        _ => None,
    }
}

fn extension_for_image_mime(mime_type: Option<&str>) -> Option<&'static str> {
    match mime_type?.to_ascii_lowercase().as_str() {
        "image/apng" => Some("apng"),
        "image/avif" => Some("avif"),
        "image/gif" => Some("gif"),
        "image/jpeg" | "image/jpg" => Some("jpg"),
        "image/png" => Some("png"),
        "image/svg+xml" => Some("svg"),
        "image/webp" => Some("webp"),
        _ => None,
    }
}

pub(super) fn sanitize_attachment_filename(filename: &str, mime_type: Option<&str>) -> String {
    let mut sanitized = sanitize_path_segment(filename);
    if sanitized == "item" {
        sanitized = "attachment".to_string();
    }
    if Path::new(&sanitized).extension().is_none() {
        if let Some(extension) = extension_for_image_mime(mime_type) {
            sanitized.push('.');
            sanitized.push_str(extension);
        }
    }
    sanitized
}

pub(super) fn sanitize_path_segment(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' => ch,
            _ => '_',
        })
        .collect::<String>()
        .trim_matches('.')
        .to_string();
    if sanitized.is_empty() {
        "item".to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests;
