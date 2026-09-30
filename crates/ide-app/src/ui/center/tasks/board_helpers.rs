#![allow(dead_code, reason = "retained alternate task-board helpers")]

use super::*;

pub(super) fn task_board_columns(board: &ide_core::TaskBoard) -> Vec<String> {
    let mut columns = board
        .columns
        .iter()
        .map(|column| column.name.clone())
        .collect::<Vec<_>>();
    for task in &board.issues {
        if !columns.iter().any(|column| column == &task.column) {
            columns.push(task.column.clone());
        }
    }
    columns.retain(|column| !column.trim().is_empty());
    if columns.is_empty() {
        columns.push("Issues".to_string());
    }
    columns
}

pub(super) fn task_board_header_subtitle(board: &ide_core::TaskBoard) -> String {
    let mut subtitle = format!("{} · {} issues", board.connection_name, board.issues.len());
    if let Some(label) = board
        .assignee_display_name
        .as_ref()
        .map(|label| label.trim())
        .filter(|label| !label.is_empty())
    {
        subtitle.push_str(" · assignee: ");
        subtitle.push_str(label);
        return subtitle;
    }
    if let Some(filter) = board
        .assignee_filter
        .as_ref()
        .map(|filter| filter.trim())
        .filter(|filter| !filter.is_empty())
    {
        if filter.eq_ignore_ascii_case("currentUser()")
            || filter.eq_ignore_ascii_case("me")
            || filter.eq_ignore_ascii_case("mine")
        {
            subtitle.push_str(" · assigned to me");
        } else {
            subtitle.push_str(" · assignee: ");
            subtitle.push_str(filter);
        }
    }
    subtitle
}

pub(super) fn task_card_meta(task: &TaskSummary) -> Vec<String> {
    let mut meta = Vec::new();
    if let Some(assignee) = &task.assignee {
        meta.push(assignee.clone());
    }
    if let Some(priority) = &task.priority {
        meta.push(priority.clone());
    }
    if meta.is_empty() {
        meta.push(task.status.clone());
    }
    meta
}

pub(super) fn task_detail_meta(task: &TaskSummary) -> Vec<String> {
    let mut meta = Vec::new();
    if let Some(issue_type) = &task.issue_type {
        meta.push(issue_type.clone());
    }
    if let Some(assignee) = &task.assignee {
        meta.push(format!("Assignee: {assignee}"));
    }
    if let Some(priority) = &task.priority {
        meta.push(format!("Priority: {priority}"));
    }
    for label in &task.labels {
        meta.push(format!("Label: {label}"));
    }
    meta
}

pub(super) fn task_element_id(task: &TaskRef) -> u64 {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    task.site_url.hash(&mut hasher);
    task.issue_key.hash(&mut hasher);
    hasher.finish()
}

pub(super) fn task_status_accent(task: &TaskSummary, cx: &mut Context<CenterArea>) -> gpui::Hsla {
    match task
        .status_category
        .as_deref()
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("done") => crate::ui::design::sage(cx),
        Some("in progress") | Some("indeterminate") => crate::ui::design::amber(cx),
        _ => crate::ui::design::t3(cx),
    }
}

/// Accent color for a board column, inferred from its name so the little status
/// dot on each column matches the meaning (done = success, in-progress/review =
/// warning, everything else = neutral). Purely presentational.
pub(super) fn task_column_accent(column: &str, cx: &mut Context<CenterArea>) -> gpui::Hsla {
    let name = column.to_ascii_lowercase();
    if name.contains("done") || name.contains("complete") || name.contains("closed") {
        crate::ui::design::sage(cx)
    } else if name.contains("progress")
        || name.contains("review")
        || name.contains("doing")
        || name.contains("testing")
    {
        crate::ui::design::amber(cx)
    } else {
        crate::ui::design::t3(cx)
    }
}
