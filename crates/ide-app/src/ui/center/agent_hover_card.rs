use super::*;
use crate::ui::{design, style};

fn linked_documents(agent: &AgentRecord) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    agent
        .source_doc
        .iter()
        .chain(&agent.linked_docs)
        .map(|path| agent.project_path.join(path))
        .filter(|path| seen.insert(path.clone()))
        .collect()
}

fn linked_tasks(agent: &AgentRecord) -> Vec<TaskRef> {
    let mut tasks: Vec<TaskRef> = Vec::new();
    for task in agent.source_task.iter().chain(&agent.linked_tasks) {
        if !tasks.iter().any(|existing| existing.same_issue(task)) {
            tasks.push(task.clone());
        }
    }
    tasks
}

impl CenterArea {
    /// Read existing context only. PR refresh uses the same asynchronous,
    /// throttled lookup as the agent header, and starts only after hover dwell.
    pub(crate) fn agent_hover_links(
        &mut self,
        agent: &AgentRecord,
        cx: &mut Context<Self>,
    ) -> Vec<gpui::AnyElement> {
        let mut links = Vec::new();
        let project = agent.project_id;
        let agent_id = agent.id;
        if !self.agent_ship_pr_targets.contains_key(&agent_id) {
            if let (Some(repo), Some(branch)) = (&agent.ship_pr_repo_path, &agent.ship_pr_branch) {
                // Hydrate the lookup cache without rewriting the persisted agent.
                self.agent_ship_pr_targets
                    .insert(agent_id, (repo.clone(), branch.clone()));
            }
        }
        self.sync_agent_ship_pull_request(agent_id, cx);
        if let Some(pr) = self.agent_ship_prs.get(&agent_id) {
            let (status, color) = crate::ui::git::git_panel::pull_request_status_style(pr, cx);
            let url = pr.url.clone();
            if !url.is_empty() {
                links.push(
                    style::hover_card_link_button(
                        "hover-agent-pr",
                        Icon::empty().path("icons/branch.svg").text_color(color),
                        format!("#{} {}", pr.number, pr.title),
                        format!("Pull request · {status}"),
                        cx,
                    )
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        crate::ui::git::git_panel::open_url(&url);
                    })
                    .into_any_element(),
                );
            }
        } else if let Some(branch) = &agent.ship_pr_branch {
            links.push(
                style::hover_card_link_button(
                    "hover-agent-pr-pending",
                    Icon::empty()
                        .path("icons/branch.svg")
                        .text_color(design::t3(cx)),
                    branch.clone(),
                    if self.agent_ship_pr_fetching.contains(&agent_id) {
                        "Checking pull request…"
                    } else {
                        "Pull request unavailable · Open agent"
                    },
                    cx,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.workspace
                        .update(cx, |workspace, cx| workspace.set_active(project, cx));
                    this.open_agent(agent_id, window, cx);
                }))
                .into_any_element(),
            );
        }

        for (index, task) in linked_tasks(agent).into_iter().enumerate() {
            let title = if task.title.trim().is_empty() {
                task.issue_key.clone()
            } else {
                task.title.clone()
            };
            links.push(
                style::hover_card_link_button(
                    ("hover-agent-task", index),
                    Icon::new(design::tasks_icon()).text_color(design::accent(cx)),
                    title,
                    format!("Task · {}", task.issue_key),
                    cx,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.open_task(project, task.clone(), cx);
                }))
                .into_any_element(),
            );
        }

        if let Some(AgentOrigin::PocketComet {
            workspace_id,
            project_id,
            task_id,
            task_title,
        }) = &agent.origin
        {
            if let Ok(mut url) = url::Url::parse("pocketcomet://task") {
                if let Ok(mut segments) = url.path_segments_mut() {
                    segments.push(task_id);
                }
                url.query_pairs_mut()
                    .append_pair("workspace_id", workspace_id)
                    .append_pair("project_id", project_id);
                let url = url.to_string();
                links.push(
                    style::hover_card_link_button(
                        "hover-agent-pocketcomet",
                        Icon::new(design::tasks_icon()).text_color(design::accent(cx)),
                        if task_title.trim().is_empty() {
                            "PocketComet task".into()
                        } else {
                            task_title.clone()
                        },
                        "PocketComet · Source task",
                        cx,
                    )
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        crate::ui::git::git_panel::open_url(&url);
                    })
                    .into_any_element(),
                );
            }
        }

        for (index, absolute) in linked_documents(agent).into_iter().enumerate() {
            let title = absolute
                .file_stem()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Document".into());
            let relative = absolute
                .strip_prefix(&agent.project_path)
                .unwrap_or(&absolute);
            links.push(
                style::hover_card_link_button(
                    ("hover-agent-doc", index),
                    Icon::new(design::docs_icon()).text_color(design::sky(cx)),
                    title,
                    format!("Document · {}", relative.display()),
                    cx,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.open_doc(project, absolute.clone(), cx);
                }))
                .into_any_element(),
            );
        }

        links
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent() -> AgentRecord {
        AgentRecord::new(
            ProjectId::new(),
            PathBuf::from("/project"),
            "Test",
            "",
            AgentKind::Codex,
            AgentModel::default_for(AgentKind::Codex),
            AgentEffort::default(),
            AgentAccessMode::default(),
        )
    }

    #[test]
    fn source_documents_are_included_once_and_resolve_from_project_not_solo() {
        let mut agent = agent();
        agent.lane_path = Some(PathBuf::from("/solo"));
        agent.source_doc = Some(PathBuf::from("docs/spec.md"));
        agent.linked_docs = vec![
            PathBuf::from("docs/spec.md"),
            PathBuf::from("/project/docs/spec.md"),
            PathBuf::from("docs/notes.md"),
        ];
        assert_eq!(
            linked_documents(&agent),
            vec![
                PathBuf::from("/project/docs/spec.md"),
                PathBuf::from("/project/docs/notes.md")
            ]
        );
    }

    #[test]
    fn source_task_and_case_variant_link_render_once() {
        let mut agent = agent();
        let task = TaskRef {
            provider: ide_core::IssueTrackerProvider::Jira,
            site_url: "https://example.atlassian.net".into(),
            issue_id: "1".into(),
            issue_key: "APP-1".into(),
            issue_url: "https://example.atlassian.net/browse/APP-1".into(),
            title: "Source task".into(),
        };
        agent.source_task = Some(task.clone());
        let mut duplicate = task.clone();
        duplicate.issue_key = "app-1".into();
        agent.linked_tasks = vec![duplicate];
        assert_eq!(linked_tasks(&agent), vec![task]);
    }
}
