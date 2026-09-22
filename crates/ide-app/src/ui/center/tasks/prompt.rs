use super::*;

pub(super) fn task_implementation_prompt(
    summary: &TaskSummary,
    detail: Option<&TaskDetail>,
) -> String {
    task_prompt(summary, detail, false)
}

pub(in crate::ui::center) fn task_design_prompt(
    summary: &TaskSummary,
    detail: Option<&TaskDetail>,
) -> String {
    task_prompt(summary, detail, true)
}

fn task_prompt(summary: &TaskSummary, detail: Option<&TaskDetail>, design: bool) -> String {
    let mut prompt = String::new();
    let provider = summary.reference.provider.label();
    if design {
        prompt.push_str(&format!(
            "Design the product experience for {} task {}: {}.\n\n",
            provider, summary.reference.issue_key, summary.reference.title
        ));
    } else {
        prompt.push_str(&format!(
            "Implement {} task {}: {}.\n\n",
            provider, summary.reference.issue_key, summary.reference.title
        ));
    }
    prompt.push_str(&format!(
        "For the freshest details — latest description, status, and comments — call the `task_read` tool with `{}`. \
         The snapshot below is a fallback if that tool isn't available; prefer the live read when you can.\n\n",
        summary.reference.issue_key
    ));
    if !summary.reference.issue_url.trim().is_empty() {
        prompt.push_str(&format!("Task URL: {}\n", summary.reference.issue_url));
    }
    prompt.push_str(&format!("Status: {}\n", summary.status));
    if let Some(assignee) = &summary.assignee {
        prompt.push_str(&format!("Assignee: {assignee}\n"));
    }
    if let Some(priority) = &summary.priority {
        prompt.push_str(&format!("Priority: {priority}\n"));
    }
    if !summary.labels.is_empty() {
        prompt.push_str(&format!("Labels: {}\n", summary.labels.join(", ")));
    }

    let description = detail
        .map(|detail| detail.description.text.trim())
        .filter(|description| !description.is_empty())
        .unwrap_or("");
    if !description.is_empty() {
        prompt.push_str("\nDescription:\n");
        prompt.push_str(description);
        prompt.push('\n');
    }

    let comments = detail
        .map(|detail| detail.comments.as_slice())
        .unwrap_or_default();
    if !comments.is_empty() {
        prompt.push_str("\nComments:\n");
        for comment in comments {
            prompt.push_str(&format!(
                "\n{}{}:\n{}\n",
                comment.author,
                comment
                    .created
                    .as_ref()
                    .map(|created| format!(" ({created})"))
                    .unwrap_or_default(),
                comment.body.text.trim()
            ));
        }
    }

    let attachments = detail
        .map(|detail| detail.attachments.as_slice())
        .unwrap_or_default();
    if !attachments.is_empty() {
        prompt.push_str("\nImages / attachments:\n");
        for attachment in attachments {
            let location = attachment
                .local_path
                .as_ref()
                .map(|path| path.display().to_string())
                .or_else(|| attachment.content_url.clone())
                .unwrap_or_else(|| "available in the task tracker".to_string());
            let kind = attachment.mime_type.as_deref().unwrap_or("attachment");
            prompt.push_str(&format!("- {} ({kind}): {location}\n", attachment.filename));
        }
    }

    if design {
        prompt.push_str("\nUse the Studio tools to create the design in this workspace. Cover the important screens, states, hierarchy, and interactions. Keep the design grounded in the task and report ambiguity before inventing major product behavior.\n\n");
        prompt.push_str(ide_core::penpot_assistant::codebase_context_instruction());
    } else {
        prompt.push_str("\nMake the needed code changes, update or add focused tests where they matter, and report any task ambiguity before making broad assumptions.");
    }
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary() -> TaskSummary {
        TaskSummary {
            reference: TaskRef {
                provider: ide_core::IssueTrackerProvider::Jira,
                site_url: "https://example.atlassian.net".to_string(),
                issue_id: "1001".to_string(),
                issue_key: "APP-42".to_string(),
                issue_url: "https://example.atlassian.net/browse/APP-42".to_string(),
                title: "Checkout experience".to_string(),
            },
            status_id: "todo".to_string(),
            status: "To Do".to_string(),
            status_category: Some("new".to_string()),
            column: "Backlog".to_string(),
            assignee: Some("Ada".to_string()),
            priority: Some("High".to_string()),
            issue_type: Some("Story".to_string()),
            labels: vec!["checkout".to_string()],
            updated: None,
            created: None,
        }
    }

    #[test]
    fn design_prompt_requests_design_work_without_code_implementation_copy() {
        let prompt = task_design_prompt(&summary(), None);
        assert!(prompt.starts_with(
            "Design the product experience for Jira task APP-42: Checkout experience."
        ));
        assert!(prompt.contains("Use the Studio tools"));
        assert!(prompt.contains("call the `task_read` tool"));
        assert!(prompt.contains("inspect the existing product and codebase"));
        assert!(prompt.contains("Reuse and extend those conventions"));
        assert!(!prompt.contains("Make the needed code changes"));
    }
}
