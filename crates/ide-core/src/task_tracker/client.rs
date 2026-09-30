use super::*;

pub struct TaskTrackerClient {
    connection: TaskTrackerConnection,
    client: reqwest::blocking::Client,
}

impl TaskTrackerClient {
    pub fn new(connection: TaskTrackerConnection) -> Result<Self> {
        if connection.provider == IssueTrackerProvider::Personal {
            return Err(anyhow!("personal board is loaded from local storage"));
        }
        Ok(Self {
            connection,
            client: provider_http_client("choro/0.1 task-trackers")?,
        })
    }

    pub fn list_sources(&self) -> Result<Vec<TaskTrackerSource>> {
        match self.connection.provider {
            IssueTrackerProvider::Jira => Ok(JiraClient::new(self.connection.clone())?
                .list_boards()?
                .into_iter()
                .map(Into::into)
                .collect()),
            IssueTrackerProvider::Linear => self.list_linear_teams(),
            IssueTrackerProvider::ClickUp | IssueTrackerProvider::Asana => {
                let source_id = self
                    .connection
                    .selected_source_id()
                    .ok_or_else(|| anyhow!("enter a source id first"))?;
                let name = self
                    .connection
                    .selected_source_name()
                    .unwrap_or_else(|| source_id.clone());
                let source_type = match self.connection.provider {
                    IssueTrackerProvider::ClickUp => "list",
                    IssueTrackerProvider::Asana => "project",
                    _ => "source",
                };
                Ok(vec![TaskTrackerSource::new(source_id, name, source_type)])
            }
            IssueTrackerProvider::PocketComet => Ok(vec![TaskTrackerSource::new(
                self.connection.selected_source_id().unwrap_or_default(),
                self.connection
                    .selected_source_name()
                    .unwrap_or_else(|| "PocketComet".to_string()),
                "project",
            )]),
            IssueTrackerProvider::Personal => unreachable!(),
        }
    }

    pub fn list_assignees(&self) -> Result<Vec<TaskTrackerUser>> {
        match self.connection.provider {
            IssueTrackerProvider::Jira => {
                let board_id = self
                    .connection
                    .board_id
                    .ok_or_else(|| anyhow!("choose a Jira board before loading assignees"))?;
                JiraClient::new(self.connection.clone())?.list_board_assignees(board_id)
            }
            IssueTrackerProvider::Linear
            | IssueTrackerProvider::ClickUp
            | IssueTrackerProvider::Asana => {
                // No convenient per-source user endpoint here — derive the
                // assignee list from the board's own (unfiltered) tasks.
                let board = match self.connection.provider {
                    IssueTrackerProvider::Linear => self.load_linear_board()?,
                    IssueTrackerProvider::ClickUp => self.load_clickup_board()?,
                    IssueTrackerProvider::Asana => self.load_asana_board()?,
                    _ => return Ok(Vec::new()),
                };
                let mut seen = std::collections::HashSet::new();
                let mut users = board
                    .issues
                    .into_iter()
                    .filter_map(|issue| {
                        let name = issue.assignee?.trim().to_string();
                        (!name.is_empty() && seen.insert(name.clone())).then(|| TaskTrackerUser {
                            account_id: name.clone(),
                            display_name: name,
                            email: None,
                            avatar_url: None,
                            active: true,
                        })
                    })
                    .collect::<Vec<_>>();
                users.sort_by(|a, b| a.display_name.cmp(&b.display_name));
                Ok(users)
            }
            IssueTrackerProvider::PocketComet => {
                Ok(PocketCometTaskSourceSnapshot::from_connection(&self.connection)?.users())
            }
            IssueTrackerProvider::Personal => Ok(Vec::new()),
        }
    }

    pub fn load_board(&self) -> Result<TaskBoard> {
        let mut board = match self.connection.provider {
            IssueTrackerProvider::Jira => JiraClient::new(self.connection.clone())?.load_board()?,
            IssueTrackerProvider::Linear => self.load_linear_board()?,
            IssueTrackerProvider::ClickUp => self.load_clickup_board()?,
            IssueTrackerProvider::Asana => self.load_asana_board()?,
            IssueTrackerProvider::PocketComet => {
                PocketCometTaskSourceSnapshot::from_connection(&self.connection)?
                    .board(&self.connection)
            }
            IssueTrackerProvider::Personal => unreachable!(),
        };
        // Jira narrows server-side via JQL and PocketComet narrows its local
        // snapshot by stable assignee id. The remaining providers fetch a whole
        // board, so filter those by the chosen display name here.
        if !matches!(
            self.connection.provider,
            IssueTrackerProvider::Jira | IssueTrackerProvider::PocketComet
        ) {
            if let Some(name) = self
                .connection
                .assignee_display_name
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty())
            {
                board
                    .issues
                    .retain(|issue| issue.assignee.as_deref().map(str::trim) == Some(name));
            }
        }
        Ok(board)
    }

    pub fn load_task_detail(
        &self,
        reference: &TaskRef,
        columns: &[TaskBoardColumn],
    ) -> Result<TaskDetail> {
        match self.connection.provider {
            IssueTrackerProvider::Jira => JiraClient::new(self.connection.clone())?
                .load_issue_detail(&reference.issue_key, columns),
            IssueTrackerProvider::Linear => self.load_linear_detail(reference, columns),
            IssueTrackerProvider::ClickUp => self.load_clickup_detail(reference, columns),
            IssueTrackerProvider::Asana => self.load_asana_detail(reference, columns),
            IssueTrackerProvider::PocketComet => {
                PocketCometTaskSourceSnapshot::from_connection(&self.connection)?.detail(reference)
            }
            IssueTrackerProvider::Personal => unreachable!(),
        }
    }

    fn list_linear_teams(&self) -> Result<Vec<TaskTrackerSource>> {
        let value = self.linear_graphql(
            r#"
            query Teams {
              teams(first: 100) {
                nodes { id name key }
              }
            }
            "#,
            serde_json::json!({}),
        )?;
        Ok(value
            .pointer("/data/teams/nodes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|team| {
                let id = string_field(team, "id")?;
                let name = string_field(team, "name").unwrap_or_else(|| id.clone());
                Some(TaskTrackerSource::new(id, name, "team"))
            })
            .collect())
    }

    fn load_linear_board(&self) -> Result<TaskBoard> {
        let team_id = self
            .connection
            .selected_source_id()
            .ok_or_else(|| anyhow!("Linear team is not configured"))?;
        let value = self.linear_graphql(
            r#"
            query TeamIssues($teamId: String!) {
              team(id: $teamId) {
                id
                name
                states(first: 100) { nodes { id name type position } }
                issues(first: 100, orderBy: updatedAt) {
                  nodes {
                    id identifier title url description priorityLabel
                    createdAt updatedAt
                    state { id name type }
                    assignee { name }
                    labels(first: 25) { nodes { name } }
                  }
                }
              }
            }
            "#,
            serde_json::json!({ "teamId": team_id }),
        )?;
        let team = value
            .pointer("/data/team")
            .ok_or_else(|| anyhow!("Linear team response missing team"))?;
        let board_name = string_field(team, "name")
            .or_else(|| self.connection.selected_source_name())
            .unwrap_or_else(|| "Linear Team".to_string());
        let columns = team
            .pointer("/states/nodes")
            .and_then(Value::as_array)
            .map(|states| {
                states
                    .iter()
                    .filter_map(|state| {
                        Some(TaskBoardColumn {
                            name: string_field(state, "name")?,
                            status_ids: vec![string_field(state, "id")?],
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let issues = team
            .pointer("/issues/nodes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|issue| linear_issue_summary(issue).ok())
            .collect::<Vec<_>>();
        Ok(TaskBoard {
            connection_id: self.connection.id,
            provider: self.connection.provider,
            connection_name: self.connection.name.clone(),
            source_id: self.connection.selected_source_id().unwrap_or_default(),
            source_name: board_name.clone(),
            board_id: 0,
            board_name,
            assignee_filter: None,
            assignee_display_name: None,
            columns: ensure_columns(columns, &issues),
            issues,
        })
    }

    fn load_linear_detail(
        &self,
        reference: &TaskRef,
        columns: &[TaskBoardColumn],
    ) -> Result<TaskDetail> {
        let value = self.linear_graphql(
            r#"
            query Issue($id: String!) {
              issue(id: $id) {
                id identifier title url description priorityLabel
                createdAt updatedAt
                state { id name type }
                assignee { name }
                labels(first: 25) { nodes { name } }
                comments(first: 50) {
                  nodes { body createdAt user { name } }
                }
              }
            }
            "#,
            serde_json::json!({ "id": reference.issue_id }),
        )?;
        let issue = value
            .pointer("/data/issue")
            .ok_or_else(|| anyhow!("Linear issue response missing issue"))?;
        let mut summary = linear_issue_summary(issue)?;
        summary.column = column_for_status(columns, &summary.status_id)
            .unwrap_or_else(|| summary.status.clone());
        let description =
            TaskRichText::plain(string_field(issue, "description").unwrap_or_default());
        let comments = issue
            .pointer("/comments/nodes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|comment| {
                let body = string_field(comment, "body").unwrap_or_default();
                if body.trim().is_empty() {
                    return None;
                }
                Some(TaskComment {
                    author: comment
                        .get("user")
                        .and_then(|user| string_field(user, "name"))
                        .unwrap_or_else(|| "Unknown".to_string()),
                    body: TaskRichText::plain(body),
                    created: string_field(comment, "createdAt"),
                })
            })
            .collect();
        Ok(TaskDetail {
            summary,
            description,
            comments,
            attachments: Vec::new(),
        })
    }

    fn load_clickup_board(&self) -> Result<TaskBoard> {
        let list_id = self
            .connection
            .selected_source_id()
            .ok_or_else(|| anyhow!("ClickUp list is not configured"))?;
        let list = self.clickup_get_json(&format!("/api/v2/list/{list_id}"), &[])?;
        let board_name = string_field(&list, "name")
            .or_else(|| self.connection.selected_source_name())
            .unwrap_or_else(|| format!("ClickUp List {list_id}"));
        let columns = list
            .get("statuses")
            .and_then(Value::as_array)
            .map(|statuses| {
                statuses
                    .iter()
                    .filter_map(|status| {
                        let name = string_field(status, "status")?;
                        Some(TaskBoardColumn {
                            status_ids: vec![name.clone()],
                            name,
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let tasks = self.clickup_get_json(
            &format!("/api/v2/list/{list_id}/task"),
            &[
                ("include_markdown_description", "true".to_string()),
                ("include_closed", "true".to_string()),
                ("subtasks", "true".to_string()),
            ],
        )?;
        let issues = tasks
            .get("tasks")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|task| clickup_task_summary(task, &self.connection).ok())
            .collect::<Vec<_>>();
        Ok(TaskBoard {
            connection_id: self.connection.id,
            provider: self.connection.provider,
            connection_name: self.connection.name.clone(),
            source_id: list_id,
            source_name: board_name.clone(),
            board_id: 0,
            board_name,
            assignee_filter: None,
            assignee_display_name: None,
            columns: ensure_columns(columns, &issues),
            issues,
        })
    }

    fn load_clickup_detail(
        &self,
        reference: &TaskRef,
        columns: &[TaskBoardColumn],
    ) -> Result<TaskDetail> {
        let value = self.clickup_get_json(
            &format!("/api/v2/task/{}", reference.issue_id),
            &[("include_markdown_description", "true".to_string())],
        )?;
        let mut summary = clickup_task_summary(&value, &self.connection)?;
        summary.column = column_for_status(columns, &summary.status_id)
            .unwrap_or_else(|| summary.status.clone());
        Ok(TaskDetail {
            description: TaskRichText::plain(
                string_field(&value, "markdown_description")
                    .or_else(|| string_field(&value, "description"))
                    .unwrap_or_default(),
            ),
            comments: self.load_clickup_comments(&reference.issue_id),
            attachments: clickup_attachments(&value),
            summary,
        })
    }

    /// Best-effort: a failed comment fetch shouldn't sink the whole task detail.
    fn load_clickup_comments(&self, task_id: &str) -> Vec<TaskComment> {
        self.clickup_get_json(&format!("/api/v2/task/{task_id}/comment"), &[])
            .ok()
            .and_then(|value| value.get("comments").and_then(Value::as_array).cloned())
            .into_iter()
            .flatten()
            .filter_map(|comment| {
                let body = string_field(&comment, "comment_text")
                    .filter(|text| !text.trim().is_empty())?;
                Some(TaskComment {
                    author: comment
                        .get("user")
                        .and_then(|user| string_field(user, "username"))
                        .unwrap_or_else(|| "Unknown".to_string()),
                    body: TaskRichText::plain(body),
                    created: string_field(&comment, "date"),
                })
            })
            .collect()
    }

    fn load_asana_board(&self) -> Result<TaskBoard> {
        let project_gid = self
            .connection
            .selected_source_id()
            .ok_or_else(|| anyhow!("Asana project is not configured"))?;
        let value = self.asana_get_json(
            &format!("/api/1.0/projects/{project_gid}/tasks"),
            &[(
                "opt_fields",
                "gid,name,completed,created_at,modified_at,assignee.name,permalink_url,notes,memberships.section.name,tags.name"
                    .to_string(),
            )],
        )?;
        let issues = value
            .get("data")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|task| asana_task_summary(task, &self.connection).ok())
            .collect::<Vec<_>>();
        let board_name = self
            .connection
            .selected_source_name()
            .unwrap_or_else(|| format!("Asana Project {project_gid}"));
        let columns = ensure_columns(Vec::new(), &issues);
        Ok(TaskBoard {
            connection_id: self.connection.id,
            provider: self.connection.provider,
            connection_name: self.connection.name.clone(),
            source_id: project_gid,
            source_name: board_name.clone(),
            board_id: 0,
            board_name,
            assignee_filter: None,
            assignee_display_name: None,
            columns,
            issues,
        })
    }

    fn load_asana_detail(
        &self,
        reference: &TaskRef,
        columns: &[TaskBoardColumn],
    ) -> Result<TaskDetail> {
        let value = self.asana_get_json(
            &format!("/api/1.0/tasks/{}", reference.issue_id),
            &[(
                "opt_fields",
                "gid,name,completed,created_at,modified_at,assignee.name,permalink_url,notes,html_notes,memberships.section.name,tags.name"
                    .to_string(),
            )],
        )?;
        let task = value
            .get("data")
            .ok_or_else(|| anyhow!("Asana task response missing data"))?;
        let mut summary = asana_task_summary(task, &self.connection)?;
        summary.column = column_for_status(columns, &summary.status_id)
            .unwrap_or_else(|| summary.status.clone());
        Ok(TaskDetail {
            summary,
            description: TaskRichText::plain(string_field(task, "notes").unwrap_or_default()),
            comments: self.load_asana_comments(&reference.issue_id),
            attachments: Vec::new(),
        })
    }

    /// Asana surfaces comments as "stories" of `type == "comment"`. Best-effort.
    fn load_asana_comments(&self, task_gid: &str) -> Vec<TaskComment> {
        self.asana_get_json(
            &format!("/api/1.0/tasks/{task_gid}/stories"),
            &[(
                "opt_fields",
                "type,text,created_by.name,created_at".to_string(),
            )],
        )
        .ok()
        .and_then(|value| value.get("data").and_then(Value::as_array).cloned())
        .into_iter()
        .flatten()
        .filter_map(|story| {
            if string_field(&story, "type").as_deref() != Some("comment") {
                return None;
            }
            let text = string_field(&story, "text").filter(|text| !text.trim().is_empty())?;
            Some(TaskComment {
                author: story
                    .get("created_by")
                    .and_then(|user| string_field(user, "name"))
                    .unwrap_or_else(|| "Unknown".to_string()),
                body: TaskRichText::plain(text),
                created: string_field(&story, "created_at"),
            })
        })
        .collect()
    }

    fn linear_graphql(&self, query: &str, variables: Value) -> Result<Value> {
        let response = self
            .client
            .post(linear_api_url(&self.connection))
            // Linear personal API keys go in the Authorization header *raw* — no
            // "Bearer " prefix (that's OAuth-only), or the API 400s.
            .header(
                reqwest::header::AUTHORIZATION,
                self.connection.expanded_api_token()?,
            )
            .header(reqwest::header::ACCEPT, "application/json")
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(serde_json::json!({ "query": query, "variables": variables }).to_string())
            .send()
            .context("Linear request failed")?;
        response_json(response, "Linear", &self.connection)
    }

    fn clickup_get_json(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        let response = self
            .client
            .get(format!("{}{}", clickup_api_base(&self.connection), path))
            .header(
                reqwest::header::AUTHORIZATION,
                self.connection.expanded_api_token()?,
            )
            .header(reqwest::header::ACCEPT, "application/json")
            .query(query)
            .send()
            .context("ClickUp request failed")?;
        response_json(response, "ClickUp", &self.connection)
    }

    fn asana_get_json(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        let response = self
            .client
            .get(format!("{}{}", asana_api_base(&self.connection), path))
            .bearer_auth(self.connection.expanded_api_token()?)
            .header(reqwest::header::ACCEPT, "application/json")
            .query(query)
            .send()
            .context("Asana request failed")?;
        response_json(response, "Asana", &self.connection)
    }

    /// The statuses a task can be moved to. For Jira these are the issue's live
    /// workflow transitions; for Linear/ClickUp the source's configured states.
    /// Asana (section-based) and Personal return empty here — the caller hides
    /// the status control for the former and handles Personal locally.
    pub fn available_statuses(&self, reference: &TaskRef) -> Result<Vec<TaskStatusOption>> {
        match self.connection.provider {
            IssueTrackerProvider::Jira => JiraClient::new(self.connection.clone())?
                .available_transitions(&reference.issue_key),
            IssueTrackerProvider::Linear => self.linear_workflow_states(),
            IssueTrackerProvider::ClickUp => self.clickup_statuses(),
            IssueTrackerProvider::PocketComet => Ok(
                PocketCometTaskSourceSnapshot::from_connection(&self.connection)?
                    .statuses
                    .into_iter()
                    .map(|status| TaskStatusOption {
                        apply_id: status.id,
                        name: status.name,
                        category: Some(status.category),
                    })
                    .collect(),
            ),
            IssueTrackerProvider::Asana | IssueTrackerProvider::Personal => Ok(Vec::new()),
        }
    }

    pub fn add_comment(&self, reference: &TaskRef, body: &str) -> Result<()> {
        match self.connection.provider {
            IssueTrackerProvider::Jira => {
                JiraClient::new(self.connection.clone())?.add_comment(&reference.issue_key, body)
            }
            IssueTrackerProvider::Linear => self.linear_add_comment(&reference.issue_id, body),
            IssueTrackerProvider::ClickUp => self.clickup_add_comment(&reference.issue_id, body),
            IssueTrackerProvider::Asana => self.asana_add_comment(&reference.issue_id, body),
            IssueTrackerProvider::PocketComet => {
                let body = body.trim();
                if body.is_empty() || body.len() > 20_000 {
                    return Err(anyhow!(
                        "PocketComet comments must be between 1 and 20,000 characters"
                    ));
                }
                let source = PocketCometTaskSourceSnapshot::from_connection(&self.connection)?;
                enqueue_pocketcomet_task_action(
                    &source,
                    &reference.issue_id,
                    PocketCometTaskActionCommand::AddComment { body: body.into() },
                )
            }
            IssueTrackerProvider::Personal => {
                Err(anyhow!("personal board comments are stored locally"))
            }
        }
    }

    pub fn set_status(&self, reference: &TaskRef, apply_id: &str) -> Result<()> {
        match self.connection.provider {
            IssueTrackerProvider::Jira => JiraClient::new(self.connection.clone())?
                .apply_transition(&reference.issue_key, apply_id),
            IssueTrackerProvider::Linear => self.linear_set_state(&reference.issue_id, apply_id),
            IssueTrackerProvider::ClickUp => self.clickup_set_status(&reference.issue_id, apply_id),
            IssueTrackerProvider::Asana => Err(anyhow!(
                "changing Asana status from here isn't supported yet"
            )),
            IssueTrackerProvider::PocketComet => {
                let source = PocketCometTaskSourceSnapshot::from_connection(&self.connection)?;
                if !source.statuses.iter().any(|status| status.id == apply_id) {
                    return Err(anyhow!("PocketComet status is no longer available"));
                }
                enqueue_pocketcomet_task_action(
                    &source,
                    &reference.issue_id,
                    PocketCometTaskActionCommand::SetStatus {
                        status_id: apply_id.to_string(),
                    },
                )
            }
            IssueTrackerProvider::Personal => {
                Err(anyhow!("personal board status is stored locally"))
            }
        }
    }

    /// Download the raw bytes of a task attachment / inline image. Jira URLs are
    /// on the authenticated site (basic auth); other providers hand out
    /// presigned CDN links that a plain GET resolves.
    pub fn download_attachment_bytes(&self, url: &str) -> Result<Vec<u8>> {
        match self.connection.provider {
            IssueTrackerProvider::Jira => {
                JiraClient::new(self.connection.clone())?.get_bytes_url(url)
            }
            IssueTrackerProvider::Personal => {
                Err(anyhow!("personal tasks have no remote attachments"))
            }
            IssueTrackerProvider::PocketComet => Err(anyhow!(
                "PocketComet task attachments are already mirrored into Choro local storage"
            )),
            _ => {
                let response = self
                    .client
                    .get(url)
                    .send()
                    .context("attachment download failed")?;
                let status = response.status();
                if !status.is_success() {
                    let text = response.text().unwrap_or_default();
                    let text = self.connection.redact_diagnostic(&text);
                    return Err(anyhow!("attachment download failed with {status}: {text}"));
                }
                Ok(response
                    .bytes()
                    .context("failed to read attachment bytes")?
                    .to_vec())
            }
        }
    }

    /// Move a task to a status identified by its display name, resolving the
    /// provider-specific token (Jira transition id, Linear state id, …) as
    /// needed. Returns the canonical status name that was applied.
    pub fn set_status_by_name(&self, reference: &TaskRef, status_name: &str) -> Result<String> {
        let wanted = status_name.trim();
        let options = self.available_statuses(reference)?;
        let target = options
            .iter()
            .find(|option| option.name.eq_ignore_ascii_case(wanted))
            .ok_or_else(|| {
                anyhow!("status \"{wanted}\" isn't available for this task right now")
            })?;
        self.set_status(reference, &target.apply_id)?;
        Ok(target.name.clone())
    }

    fn linear_workflow_states(&self) -> Result<Vec<TaskStatusOption>> {
        let team_id = self
            .connection
            .selected_source_id()
            .ok_or_else(|| anyhow!("Linear team is not configured"))?;
        let value = self.linear_graphql(
            "query($team: String!) { team(id: $team) { states(first: 100) { nodes { id name type position } } } }",
            serde_json::json!({ "team": team_id }),
        )?;
        let mut nodes = value
            .pointer("/data/team/states/nodes")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        nodes.sort_by(|a, b| {
            let ap = a.get("position").and_then(Value::as_f64).unwrap_or(0.0);
            let bp = b.get("position").and_then(Value::as_f64).unwrap_or(0.0);
            ap.partial_cmp(&bp).unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(nodes
            .into_iter()
            .filter_map(|state| {
                Some(TaskStatusOption {
                    apply_id: string_field(&state, "id")?,
                    name: string_field(&state, "name")?,
                    category: string_field(&state, "type").map(linear_state_category),
                })
            })
            .collect())
    }

    fn linear_add_comment(&self, issue_id: &str, body: &str) -> Result<()> {
        let value = self.linear_graphql(
            "mutation($id: String!, $body: String!) { commentCreate(input: { issueId: $id, body: $body }) { success } }",
            serde_json::json!({ "id": issue_id, "body": body }),
        )?;
        ensure_linear_success(
            &value,
            "/data/commentCreate/success",
            "post the Linear comment",
        )
    }

    fn linear_set_state(&self, issue_id: &str, state_id: &str) -> Result<()> {
        let value = self.linear_graphql(
            "mutation($id: String!, $state: String!) { issueUpdate(id: $id, input: { stateId: $state }) { success } }",
            serde_json::json!({ "id": issue_id, "state": state_id }),
        )?;
        ensure_linear_success(
            &value,
            "/data/issueUpdate/success",
            "update the Linear status",
        )
    }

    fn clickup_statuses(&self) -> Result<Vec<TaskStatusOption>> {
        let list_id = self
            .connection
            .selected_source_id()
            .ok_or_else(|| anyhow!("ClickUp list is not configured"))?;
        let value = self.clickup_get_json(&format!("/api/v2/list/{list_id}"), &[])?;
        Ok(value
            .get("statuses")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|status| {
                let name = string_field(status, "status")?;
                Some(TaskStatusOption {
                    apply_id: name.clone(),
                    name,
                    category: string_field(status, "type"),
                })
            })
            .collect())
    }

    fn clickup_add_comment(&self, task_id: &str, body: &str) -> Result<()> {
        self.clickup_send_json(
            reqwest::Method::POST,
            &format!("/api/v2/task/{task_id}/comment"),
            serde_json::json!({ "comment_text": body, "notify_all": false }),
        )
        .map(|_| ())
    }

    fn clickup_set_status(&self, task_id: &str, status: &str) -> Result<()> {
        self.clickup_send_json(
            reqwest::Method::PUT,
            &format!("/api/v2/task/{task_id}"),
            serde_json::json!({ "status": status }),
        )
        .map(|_| ())
    }

    fn asana_add_comment(&self, task_gid: &str, body: &str) -> Result<()> {
        self.asana_send_json(
            &format!("/api/1.0/tasks/{task_gid}/stories"),
            serde_json::json!({ "data": { "text": body } }),
        )
        .map(|_| ())
    }

    fn clickup_send_json(&self, method: reqwest::Method, path: &str, body: Value) -> Result<Value> {
        let response = self
            .client
            .request(
                method,
                format!("{}{}", clickup_api_base(&self.connection), path),
            )
            .header(
                reqwest::header::AUTHORIZATION,
                self.connection.expanded_api_token()?,
            )
            .header(reqwest::header::ACCEPT, "application/json")
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body.to_string())
            .send()
            .context("ClickUp write request failed")?;
        response_json(response, "ClickUp", &self.connection)
    }

    fn asana_send_json(&self, path: &str, body: Value) -> Result<Value> {
        let response = self
            .client
            .post(format!("{}{}", asana_api_base(&self.connection), path))
            .bearer_auth(self.connection.expanded_api_token()?)
            .header(reqwest::header::ACCEPT, "application/json")
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body.to_string())
            .send()
            .context("Asana write request failed")?;
        response_json(response, "Asana", &self.connection)
    }
}
