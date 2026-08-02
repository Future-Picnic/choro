use super::*;

pub(super) fn selected_assignee_label(row: &super::connection_editor::TaskConnectionRow) -> String {
    row.assignee_display_name
        .as_ref()
        .map(|name| name.trim())
        .filter(|name| !name.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            row.assignee_account_id
                .as_ref()
                .map(|account_id| account_id.trim())
                .filter(|account_id| !account_id.is_empty())
                .map(ToString::to_string)
        })
        .or_else(|| {
            row.assignee_filter
                .as_ref()
                .map(|filter| filter.trim())
                .filter(|filter| !filter.is_empty())
                .map(|filter| format!("Legacy filter: {filter}"))
        })
        .unwrap_or_else(|| "All assignees".to_string())
}

/// Dark badge background behind each provider's brand mark (matches the design).
pub(crate) fn provider_badge_bg(provider: IssueTrackerProvider) -> gpui::Hsla {
    let key = match provider {
        IssueTrackerProvider::Jira => "jira",
        IssueTrackerProvider::Linear => "linear",
        IssueTrackerProvider::Asana => "asana",
        IssueTrackerProvider::ClickUp => "clickup",
        IssueTrackerProvider::Personal => "personal",
    };
    crate::ui::design::palette::tracker_brand(key)
}

pub(crate) fn provider_brand_asset(provider: IssueTrackerProvider) -> Option<&'static str> {
    match provider {
        IssueTrackerProvider::Jira => Some("brand/jira.svg"),
        IssueTrackerProvider::Linear => Some("brand/linear.svg"),
        IssueTrackerProvider::Asana => Some("brand/asana.svg"),
        IssueTrackerProvider::ClickUp => Some("brand/clickup.svg"),
        IssueTrackerProvider::Personal => None,
    }
}

/// A rounded brand badge: dark tile + full-colour logo (or a person glyph for Personal).
pub(crate) fn provider_badge(
    provider: IssueTrackerProvider,
    size: f32,
    cx: &App,
) -> gpui::AnyElement {
    let inner = px(size * 0.6);
    let tile = div()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .size(px(size))
        .rounded(px((size * 0.28).max(4.)))
        .bg(provider_badge_bg(provider));
    match provider_brand_asset(provider) {
        Some(path) => tile.child(img(path).w(inner).h(inner)).into_any_element(),
        None => tile
            .child(
                Icon::new(IconName::CircleUser)
                    .with_size(inner)
                    .text_color(crate::ui::design::t1(cx)),
            )
            .into_any_element(),
    }
}

pub(crate) fn board_display_name(connection: &TaskTrackerConnection) -> String {
    connection
        .board_name
        .clone()
        .or_else(|| connection.source_name.clone())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| {
            connection
                .board_id
                .map(|id| format!("Board {id}"))
                .or_else(|| {
                    connection
                        .source_id
                        .as_ref()
                        .map(|id| format!("{} {}", connection.provider.label(), id))
                })
                .unwrap_or_else(|| connection.provider.label().to_string())
        })
}

pub(crate) fn classify_status(text: &str) -> u8 {
    let text = text.to_ascii_lowercase();
    if text.contains("done")
        || text.contains("complete")
        || text.contains("closed")
        || text.contains("resolved")
    {
        2
    } else if text.contains("progress")
        || text.contains("review")
        || text.contains("doing")
        || text.contains("active")
        || text.contains("indeterminate")
    {
        1
    } else {
        0
    }
}

pub(crate) fn dot_from_class(class: u8, cx: &App) -> gpui::Hsla {
    match class {
        2 => crate::ui::design::sage(cx),  // done
        1 => crate::ui::design::sky(cx),   // in progress
        _ => crate::ui::design::amber(cx), // to do
    }
}

pub(crate) fn task_status_color(summary: &ide_core::TaskSummary, cx: &App) -> gpui::Hsla {
    status_dot_color(summary, cx)
}

pub(crate) fn status_dot_color(summary: &ide_core::TaskSummary, cx: &App) -> gpui::Hsla {
    let key = format!(
        "{} {} {}",
        summary.status_category.as_deref().unwrap_or(""),
        summary.status,
        summary.column
    );
    dot_from_class(classify_status(&key), cx)
}

pub(crate) fn column_dot_color(
    name: &str,
    sample: Option<&ide_core::TaskSummary>,
    cx: &App,
) -> gpui::Hsla {
    let mut key = name.to_string();
    if let Some(sample) = sample {
        key.push(' ');
        key.push_str(sample.status_category.as_deref().unwrap_or(""));
    }
    dot_from_class(classify_status(&key), cx)
}

pub(crate) fn task_meta_line(summary: &ide_core::TaskSummary) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(assignee) = summary
        .assignee
        .as_ref()
        .map(|assignee| assignee.trim())
        .filter(|assignee| !assignee.is_empty())
    {
        parts.push(assignee.to_string());
    }
    if let Some(priority) = summary
        .priority
        .as_ref()
        .map(|priority| priority.trim())
        .filter(|priority| !priority.is_empty())
    {
        parts.push(priority.to_string());
    }
    parts.join(" · ")
}

pub(crate) fn provider_site_placeholder(provider: IssueTrackerProvider) -> &'static str {
    match provider {
        IssueTrackerProvider::Jira => "https://your-domain.atlassian.net",
        IssueTrackerProvider::Linear => "https://api.linear.app/graphql",
        IssueTrackerProvider::ClickUp => "https://api.clickup.com",
        IssueTrackerProvider::Asana => "https://app.asana.com",
        IssueTrackerProvider::Personal => "",
    }
}

pub(crate) fn provider_token_placeholder(provider: IssueTrackerProvider) -> &'static str {
    match provider {
        IssueTrackerProvider::Jira => "${JIRA_API_TOKEN} or API token",
        IssueTrackerProvider::Linear => "${LINEAR_API_TOKEN} or personal API key",
        IssueTrackerProvider::ClickUp => "${CLICKUP_API_TOKEN} or API token",
        IssueTrackerProvider::Asana => "${ASANA_API_TOKEN} or PAT",
        IssueTrackerProvider::Personal => "",
    }
}

pub(crate) fn provider_default_token(provider: IssueTrackerProvider) -> String {
    match provider {
        IssueTrackerProvider::Jira => "${JIRA_API_TOKEN}",
        IssueTrackerProvider::Linear => "${LINEAR_API_TOKEN}",
        IssueTrackerProvider::ClickUp => "${CLICKUP_API_TOKEN}",
        IssueTrackerProvider::Asana => "${ASANA_API_TOKEN}",
        IssueTrackerProvider::Personal => "",
    }
    .to_string()
}

pub(crate) fn provider_default_site_url(provider: IssueTrackerProvider) -> String {
    match provider {
        IssueTrackerProvider::Jira => "",
        IssueTrackerProvider::Linear => "https://api.linear.app/graphql",
        IssueTrackerProvider::ClickUp => "https://api.clickup.com",
        IssueTrackerProvider::Asana => "https://app.asana.com",
        IssueTrackerProvider::Personal => "",
    }
    .to_string()
}

pub(crate) fn provider_source_id_placeholder(provider: IssueTrackerProvider) -> &'static str {
    match provider {
        IssueTrackerProvider::Jira => "board id",
        IssueTrackerProvider::Linear => "team id",
        IssueTrackerProvider::ClickUp => "list id",
        IssueTrackerProvider::Asana => "project gid",
        IssueTrackerProvider::Personal => "",
    }
}

pub(crate) fn provider_source_name_placeholder(provider: IssueTrackerProvider) -> &'static str {
    match provider {
        IssueTrackerProvider::Jira => "board name",
        IssueTrackerProvider::Linear => "team name",
        IssueTrackerProvider::ClickUp => "list name",
        IssueTrackerProvider::Asana => "project name",
        IssueTrackerProvider::Personal => "",
    }
}

pub(crate) fn provider_source_kind(provider: IssueTrackerProvider) -> &'static str {
    match provider {
        IssueTrackerProvider::Jira => "board",
        IssueTrackerProvider::Linear => "team",
        IssueTrackerProvider::ClickUp => "list",
        IssueTrackerProvider::Asana => "project",
        IssueTrackerProvider::Personal => "personal",
    }
}

pub(crate) fn form_field_label(text: impl Into<SharedString>, cx: &App) -> gpui::AnyElement {
    div()
        .text_size(crate::ui::design::text_ui())
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(crate::ui::design::t3(cx))
        .child(text.into())
        .into_any_element()
}

/// Human label for the source a provider pulls from (Jira board, Linear team …).
pub(crate) fn provider_source_label(provider: IssueTrackerProvider) -> &'static str {
    match provider {
        IssueTrackerProvider::Jira => "Board",
        IssueTrackerProvider::Linear => "Team",
        IssueTrackerProvider::ClickUp => "List",
        IssueTrackerProvider::Asana => "Project",
        IssueTrackerProvider::Personal => "Source",
    }
}

pub(crate) fn provider_help_title(provider: IssueTrackerProvider) -> &'static str {
    match provider {
        IssueTrackerProvider::Jira => "Connect Jira",
        IssueTrackerProvider::Linear => "Connect Linear",
        IssueTrackerProvider::ClickUp => "Connect ClickUp",
        IssueTrackerProvider::Asana => "Connect Asana",
        IssueTrackerProvider::Personal => "Personal Board",
    }
}

pub(crate) fn provider_help_steps(provider: IssueTrackerProvider) -> &'static [&'static str] {
    match provider {
        IssueTrackerProvider::Jira => &[
            "Enter your Atlassian site URL, Jira account email, and a current Atlassian API token created without scopes.",
            "Click Load Boards, choose the board to show in this project, then Save.",
            "Optional: after choosing a board, click Load Users and choose one assignee to filter the board.",
        ],
        IssueTrackerProvider::Linear => &[
            "Create a personal API key in Linear settings and paste it as the token.",
            "Keep the API URL as https://api.linear.app/graphql unless you use a proxy.",
            "Click Load Teams, choose the Linear team to show, then Save.",
        ],
        IssueTrackerProvider::ClickUp => &[
            "Create a ClickUp API token in ClickUp settings and paste it as the token.",
            "Paste the ClickUp List ID for the list you want this project to show.",
            "Add an optional list name, click Use Source, then Save. The MVP reads one list at a time.",
        ],
        IssueTrackerProvider::Asana => &[
            "Create an Asana personal access token and paste it as the token.",
            "Paste the Asana Project GID for the project you want this app to show.",
            "Add an optional project name, click Use Source, then Save. The MVP reads one project at a time.",
        ],
        IssueTrackerProvider::Personal => &[
            "Personal Board is built in for every project.",
            "Create and edit local tasks directly in the Tasks view.",
        ],
    }
}
