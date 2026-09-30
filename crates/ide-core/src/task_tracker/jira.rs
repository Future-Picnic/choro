use super::*;

pub struct JiraClient {
    connection: TaskTrackerConnection,
    client: reqwest::blocking::Client,
}

impl JiraClient {
    pub fn new(connection: TaskTrackerConnection) -> Result<Self> {
        if connection.provider != IssueTrackerProvider::Jira {
            return Err(anyhow!("unsupported task tracker provider"));
        }
        Ok(Self {
            connection,
            client: provider_http_client("choro/0.1 jira-tasks")
                .context("failed to build Jira HTTP client")?,
        })
    }

    pub fn list_boards(&self) -> Result<Vec<JiraBoard>> {
        let mut boards = Vec::new();
        let mut start_at = 0_i64;
        loop {
            let value = self.get_json(
                "/rest/agile/1.0/board",
                &[
                    ("startAt", start_at.to_string()),
                    ("maxResults", "50".to_string()),
                ],
            )?;
            for board in value
                .get("values")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
            {
                let Some(id) = board.get("id").and_then(Value::as_i64) else {
                    continue;
                };
                let name = string_field(&board, "name").unwrap_or_else(|| format!("Board {id}"));
                let board_type = string_field(&board, "type").unwrap_or_default();
                boards.push(JiraBoard {
                    id,
                    name,
                    board_type,
                });
            }
            let max_results = value
                .get("maxResults")
                .and_then(Value::as_i64)
                .unwrap_or(50)
                .max(1);
            let is_last = value
                .get("isLast")
                .and_then(Value::as_bool)
                .unwrap_or_else(|| {
                    value
                        .get("total")
                        .and_then(Value::as_i64)
                        .is_some_and(|total| start_at + max_results >= total)
                });
            if is_last {
                break;
            }
            start_at += max_results;
        }
        Ok(boards)
    }

    pub fn load_board(&self) -> Result<TaskBoard> {
        let board_id = self
            .connection
            .board_id
            .ok_or_else(|| anyhow!("Jira board is not configured"))?;
        let board_name = self
            .connection
            .board_name
            .clone()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| format!("Board {board_id}"));
        let columns = self.load_board_columns(board_id)?;
        let issues = self.load_board_issues(board_id, &columns)?;
        Ok(TaskBoard {
            connection_id: self.connection.id,
            provider: self.connection.provider,
            connection_name: self.connection.name.clone(),
            source_id: board_id.to_string(),
            source_name: board_name.clone(),
            board_id,
            board_name,
            assignee_filter: self.connection.assignee_filter.clone(),
            assignee_display_name: self.connection.assignee_label().map(ToString::to_string),
            columns,
            issues,
        })
    }

    pub fn list_board_assignees(&self, board_id: i64) -> Result<Vec<JiraUser>> {
        let mut assignees = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut next_page_token: Option<String> = None;
        loop {
            let mut query = vec![
                ("maxResults", "100".to_string()),
                ("fields", "assignee".to_string()),
            ];
            if let Some(token) = &next_page_token {
                query.push(("nextPageToken", token.clone()));
            }
            let value = self.get_json(
                &format!("/rest/software/1.0/board/{board_id}/issue"),
                &query,
            )?;
            for issue in value
                .get("issues")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let Some(assignee) = issue
                    .get("fields")
                    .and_then(|fields| fields.get("assignee"))
                    .filter(|assignee| !assignee.is_null())
                else {
                    continue;
                };
                let Some(user) = jira_user_from_value(assignee) else {
                    continue;
                };
                if seen.insert(user.account_id.clone()) {
                    assignees.push(user);
                }
            }
            if value.get("isLast").and_then(Value::as_bool).unwrap_or(true) {
                break;
            }
            next_page_token = string_field(&value, "nextPageToken");
            if next_page_token.is_none() {
                break;
            }
        }
        assignees.sort_by(|left, right| {
            left.display_name
                .to_ascii_lowercase()
                .cmp(&right.display_name.to_ascii_lowercase())
        });
        Ok(assignees)
    }

    pub fn load_issue_detail(
        &self,
        issue_key: &str,
        columns: &[TaskBoardColumn],
    ) -> Result<TaskDetail> {
        let value = self.get_json(
            &format!("/rest/api/3/issue/{issue_key}"),
            &[(
                "fields",
                "summary,description,comment,status,assignee,priority,issuetype,labels,updated,created,attachment"
                    .to_string(),
            )],
        )?;
        let summary = self.issue_summary_from_value(&value, columns)?;
        let fields = value.get("fields").cloned().unwrap_or(Value::Null);
        let mut attachments = parse_task_attachments(&fields);
        for attachment in &mut attachments {
            if !attachment.is_image() {
                continue;
            }
            if let Ok(materialized) =
                self.materialize_image_attachment(&summary.reference.issue_key, attachment)
            {
                *attachment = materialized;
            }
        }
        let description = fields
            .get("description")
            .map(|description| jira_adf_to_rich_text(description, &attachments))
            .unwrap_or_else(|| TaskRichText::plain(""));
        let comments = fields
            .get("comment")
            .and_then(|comment| comment.get("comments"))
            .and_then(Value::as_array)
            .map(|comments| {
                comments
                    .iter()
                    .filter_map(|comment| {
                        let author = comment
                            .get("author")
                            .and_then(|author| string_field(author, "displayName"))
                            .unwrap_or_else(|| "Unknown".to_string());
                        let body = comment
                            .get("body")
                            .map(|body| jira_adf_to_rich_text(body, &attachments))
                            .unwrap_or_else(|| TaskRichText::plain(""));
                        if body.text.trim().is_empty() && body.blocks.is_empty() {
                            return None;
                        }
                        Some(TaskComment {
                            author,
                            body,
                            created: string_field(comment, "created"),
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        Ok(TaskDetail {
            summary,
            description,
            comments,
            attachments,
        })
    }

    fn load_board_columns(&self, board_id: i64) -> Result<Vec<TaskBoardColumn>> {
        let value = self.get_json(
            &format!("/rest/agile/1.0/board/{board_id}/configuration"),
            &[],
        )?;
        let columns = value
            .get("columnConfig")
            .and_then(|config| config.get("columns"))
            .and_then(Value::as_array)
            .map(|columns| {
                columns
                    .iter()
                    .filter_map(|column| {
                        let name = string_field(column, "name")?;
                        let status_ids = column
                            .get("statuses")
                            .and_then(Value::as_array)
                            .map(|statuses| {
                                statuses
                                    .iter()
                                    .filter_map(|status| {
                                        status
                                            .get("id")
                                            .and_then(Value::as_str)
                                            .map(ToString::to_string)
                                    })
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                        Some(TaskBoardColumn { name, status_ids })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        Ok(columns)
    }

    fn load_board_issues(
        &self,
        board_id: i64,
        columns: &[TaskBoardColumn],
    ) -> Result<Vec<TaskSummary>> {
        let mut issues = Vec::new();
        let mut next_page_token: Option<String> = None;
        loop {
            let mut query = vec![
                ("maxResults", "100".to_string()),
                (
                    "fields",
                    "summary,status,assignee,priority,issuetype,labels,updated,created".to_string(),
                ),
            ];
            if let Some(token) = &next_page_token {
                query.push(("nextPageToken", token.clone()));
            }
            if let Some(jql) = self.connection.assignee_filter_jql() {
                query.push(("jql", jql));
            }
            let value = self.get_json(
                &format!("/rest/software/1.0/board/{board_id}/issue"),
                &query,
            )?;
            for issue in value
                .get("issues")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
            {
                if let Ok(summary) = self.issue_summary_from_value(&issue, columns) {
                    issues.push(summary);
                }
            }
            if value.get("isLast").and_then(Value::as_bool).unwrap_or(true) {
                break;
            }
            next_page_token = string_field(&value, "nextPageToken");
            if next_page_token.is_none() {
                break;
            }
        }
        Ok(issues)
    }

    pub(super) fn issue_summary_from_value(
        &self,
        issue: &Value,
        columns: &[TaskBoardColumn],
    ) -> Result<TaskSummary> {
        let issue_id = string_field(issue, "id").context("Jira issue missing id")?;
        let issue_key = string_field(issue, "key").context("Jira issue missing key")?;
        let fields = issue
            .get("fields")
            .ok_or_else(|| anyhow!("Jira issue {issue_key} missing fields"))?;
        let title = string_field(fields, "summary").unwrap_or_else(|| issue_key.clone());
        let status = fields.get("status").unwrap_or(&Value::Null);
        let status_id = string_field(status, "id").unwrap_or_default();
        let status_name = string_field(status, "name").unwrap_or_else(|| "No status".to_string());
        let column = column_for_status(columns, &status_id).unwrap_or_else(|| status_name.clone());
        let site_url = self.connection.normalized_site_url();
        let reference = TaskRef {
            provider: IssueTrackerProvider::Jira,
            site_url: site_url.clone(),
            issue_id,
            issue_key: issue_key.clone(),
            issue_url: format!("{site_url}/browse/{issue_key}"),
            title,
        };
        Ok(TaskSummary {
            reference,
            status_id,
            status: status_name,
            status_category: status
                .get("statusCategory")
                .and_then(|category| string_field(category, "name")),
            column,
            assignee: fields
                .get("assignee")
                .and_then(|assignee| string_field(assignee, "displayName")),
            priority: fields
                .get("priority")
                .and_then(|priority| string_field(priority, "name")),
            issue_type: fields
                .get("issuetype")
                .and_then(|issue_type| string_field(issue_type, "name")),
            labels: string_array_field(fields, "labels"),
            updated: string_field(fields, "updated"),
            created: string_field(fields, "created"),
        })
    }

    fn get_json(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        let url = format!("{}{}", self.connection.normalized_site_url(), path);
        let request = self
            .client
            .get(url)
            .basic_auth(
                self.connection.email.trim().to_string(),
                Some(self.connection.expanded_api_token()?),
            )
            .header(reqwest::header::ACCEPT, "application/json")
            .query(query);
        let response = request.send().context("Jira request failed")?;
        let status = response.status();
        let text = response.text().context("failed to read Jira response")?;
        if !status.is_success() {
            return Err(self.response_error("Jira request", status, &text));
        }
        serde_json::from_str(&text).context("failed to parse Jira response")
    }

    fn response_error(
        &self,
        action: &str,
        status: reqwest::StatusCode,
        body: &str,
    ) -> anyhow::Error {
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return anyhow!(
                "{action} authentication failed (401 Unauthorized). The API token may be expired or revoked, or the email may not match the Atlassian account that created it. Replace it with a current Atlassian API token created without scopes."
            );
        }

        let body = self.connection.redact_diagnostic(body);
        anyhow!("{action} failed with {status}: {body}")
    }

    /// Post a plain-text comment. Jira Cloud's v3 comment API expects an ADF
    /// (Atlassian Document Format) body, so wrap the text in a minimal doc.
    pub fn add_comment(&self, issue_key: &str, body: &str) -> Result<()> {
        let payload = serde_json::json!({
            "body": {
                "type": "doc",
                "version": 1,
                "content": [{
                    "type": "paragraph",
                    "content": [{ "type": "text", "text": body }]
                }]
            }
        });
        self.post_json(&format!("/rest/api/3/issue/{issue_key}/comment"), payload)
    }

    /// The transitions available for an issue *right now*. Jira status changes go
    /// through workflow transitions, so `apply_id` is the transition id and
    /// `name` is the destination status.
    pub fn available_transitions(&self, issue_key: &str) -> Result<Vec<TaskStatusOption>> {
        let value = self.get_json(&format!("/rest/api/3/issue/{issue_key}/transitions"), &[])?;
        Ok(value
            .get("transitions")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|transition| {
                let apply_id = string_field(transition, "id")?;
                let to = transition.get("to");
                let name = to
                    .and_then(|to| string_field(to, "name"))
                    .or_else(|| string_field(transition, "name"))?;
                let category = to
                    .and_then(|to| to.get("statusCategory"))
                    .and_then(|category| string_field(category, "key"));
                Some(TaskStatusOption {
                    apply_id,
                    name,
                    category,
                })
            })
            .collect())
    }

    pub fn apply_transition(&self, issue_key: &str, transition_id: &str) -> Result<()> {
        let payload = serde_json::json!({ "transition": { "id": transition_id } });
        self.post_json(
            &format!("/rest/api/3/issue/{issue_key}/transitions"),
            payload,
        )
    }

    fn post_json(&self, path: &str, body: Value) -> Result<()> {
        let url = format!("{}{}", self.connection.normalized_site_url(), path);
        let response = self
            .client
            .post(url)
            .basic_auth(
                self.connection.email.trim().to_string(),
                Some(self.connection.expanded_api_token()?),
            )
            .header(reqwest::header::ACCEPT, "application/json")
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body.to_string())
            .send()
            .context("Jira write request failed")?;
        let status = response.status();
        if !status.is_success() {
            let text = response.text().unwrap_or_default();
            return Err(self.response_error("Jira write request", status, &text));
        }
        Ok(())
    }

    pub fn get_bytes_url(&self, url: &str) -> Result<Vec<u8>> {
        let url = if url.starts_with("http://") || url.starts_with("https://") {
            url.to_string()
        } else {
            format!("{}{}", self.connection.normalized_site_url(), url)
        };
        let request = self
            .client
            .get(url)
            .basic_auth(
                self.connection.email.trim().to_string(),
                Some(self.connection.expanded_api_token()?),
            )
            .header(reqwest::header::ACCEPT, "*/*");
        let response = request.send().context("Jira attachment request failed")?;
        let status = response.status();
        if !status.is_success() {
            let text = response
                .text()
                .unwrap_or_else(|_| "failed to read Jira attachment response".to_string());
            return Err(self.response_error("Jira attachment request", status, &text));
        }
        Ok(response
            .bytes()
            .context("failed to read Jira attachment bytes")?
            .to_vec())
    }

    fn materialize_image_attachment(
        &self,
        issue_key: &str,
        attachment: &TaskAttachment,
    ) -> Result<TaskAttachment> {
        let mut last_error = None;
        let mut bytes = None;
        for url in [
            attachment.content_url.as_deref(),
            attachment.thumbnail_url.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            match self.get_bytes_url(url) {
                Ok(value) => {
                    bytes = Some(value);
                    break;
                }
                Err(error) => last_error = Some(error),
            }
        }
        let bytes = bytes.ok_or_else(|| {
            last_error.unwrap_or_else(|| anyhow!("Jira image attachment has no content URL"))
        })?;
        let directory = std::env::temp_dir()
            .join("choro-jira-task-images")
            .join(sanitize_path_segment(issue_key));
        fs::create_dir_all(&directory).context("failed to create Jira image cache directory")?;
        let filename =
            sanitize_attachment_filename(&attachment.filename, attachment.mime_type.as_deref());
        let path = directory.join(format!(
            "{}-{filename}",
            sanitize_path_segment(&attachment.id)
        ));
        fs::write(&path, bytes).with_context(|| {
            format!(
                "failed to write Jira image attachment cache {}",
                path.display()
            )
        })?;
        let mut materialized = attachment.clone();
        materialized.local_path = Some(path);
        Ok(materialized)
    }
}
