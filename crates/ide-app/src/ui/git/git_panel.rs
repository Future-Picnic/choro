use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gpui::{
    div, prelude::FluentBuilder, px, svg, uniform_list, Animation, AnimationExt, App, AppContext,
    Context, Entity, FontWeight, Hsla, InteractiveElement, IntoElement, MouseButton, ParentElement,
    Render, SharedString, StatefulInteractiveElement, Styled, Window,
};
use gpui_component::{
    button::{Button, ButtonVariants, Toggle, ToggleVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
    spinner::Spinner,
    v_flex, Disableable, IconName, Sizable, WindowExt,
};
use ide_core::{
    config::{GenerationAgent, GitStatusGroupMode, GitStatusViewMode},
    git::{BranchInfo, FileDiff, GitHubAccount, GitRemote},
    AgentKind, ProjectId,
};

use crate::notifications;
use crate::state::{AgentRecords, GitState, GitStates, Workspace};
use crate::ui::branch_icon::{branch_icon, commit_icon, pr_icon};
use crate::ui::confirm::ConfirmDialog;
use crate::ui::git::status_list::{render_section, split_entries, status_list_entries};
use crate::ui::split_button::{SplitButton, SplitPalette};

mod branches;
mod commit_area;
mod generation;
mod history;
mod panel_state;
mod pull_request_dialog;
mod pull_request_support;
mod pull_requests_view;
mod render;
mod repository_setup;
mod solo_strip;

use generation::git_output;
pub(crate) use generation::run_safe_text_generation;
pub(crate) use generation::{
    default_remote_branch, distill_memory_proposal, generate_commit_message,
    generate_commit_message_for_files, generate_one_shot_text, generate_pull_request,
    generate_pull_request_for_files, generate_riff, pull_request_base_branch_options,
    GeneratedPullRequest, MemoryDecisionContext,
};
use pull_request_dialog::{confirm_git_action, GitConfirmation};
use pull_request_support::{
    branch_from_push_message, existing_pull_request_url, repo_pull_requests,
};
pub(crate) use pull_request_support::{
    branch_pull_request, create_pull_request_with_gh, github_pull_request_url, open_url,
    pull_request_status_style, pull_request_url_with_text,
};
use repository_setup::open_publish_repository_dialog;

const CODEX_GENERATION_REASONING_EFFORT: &str = "low";
const PULL_REQUEST_REFRESH_INTERVAL: Duration = Duration::from_secs(300);
const PULL_REQUEST_MISSING_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
static GH_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();

fn resolved_gh_path() -> Option<PathBuf> {
    GH_PATH
        .get_or_init(|| {
            for path in [
                "/opt/homebrew/bin/gh",
                "/usr/local/bin/gh",
                "/usr/bin/gh",
                "/opt/local/bin/gh",
            ] {
                let path = PathBuf::from(path);
                if path.exists() {
                    return Some(path);
                }
            }

            let output = Command::new("/bin/zsh")
                .args(["-lc", "command -v gh"])
                .env(
                    "PATH",
                    "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin",
                )
                .output()
                .ok()?;
            if !output.status.success() {
                return None;
            }
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            (!path.is_empty()).then(|| PathBuf::from(path))
        })
        .clone()
}

fn gh_command() -> anyhow::Result<Command> {
    let Some(path) = resolved_gh_path() else {
        anyhow::bail!(
            "GitHub CLI not found. Install gh, or make it available at /opt/homebrew/bin/gh or /usr/local/bin/gh."
        );
    };
    Ok(Command::new(path))
}

/// "4 hours ago"-style label from a unix timestamp.
pub(crate) fn relative_time(unix_secs: i64) -> String {
    if unix_secs <= 0 {
        return String::new();
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let delta = (now - unix_secs).max(0);
    match delta {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{}m ago", delta / 60),
        3600..=86_399 => format!("{}h ago", delta / 3600),
        86_400..=2_591_999 => format!("{}d ago", delta / 86_400),
        _ => format!("{}mo ago", delta / 2_592_000),
    }
}

/// Rough client-side validation of a git branch name.
fn is_valid_branch_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && !name.starts_with('/')
        && !name.ends_with('/')
        && !name.ends_with('.')
        && !name.contains("..")
        && !name
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '~' | '^' | ':' | '?' | '*' | '[' | '\\'))
}

fn slug_branch_component(component: &str) -> Option<String> {
    let mut slug = String::new();
    let mut pending_separator = false;

    for ch in component.chars().flat_map(|ch| ch.to_lowercase()) {
        if ch.is_ascii_alphanumeric() {
            if pending_separator && !slug.is_empty() {
                slug.push('-');
            }
            slug.push(ch);
            pending_separator = false;
        } else {
            pending_separator = true;
        }
    }

    (!slug.is_empty()).then_some(slug)
}

fn slug_branch_name(name: &str) -> Option<String> {
    let components: Vec<String> = name
        .trim()
        .split('/')
        .filter_map(slug_branch_component)
        .collect();
    (!components.is_empty()).then(|| components.join("/"))
}

fn branch_name_candidate(query: &str) -> Option<String> {
    let query = query.trim();
    if query.is_empty() {
        return None;
    }
    if is_valid_branch_name(query) {
        return Some(query.to_string());
    }

    slug_branch_name(query).filter(|name| is_valid_branch_name(name))
}

#[cfg(test)]
mod tests {
    use super::{
        branch_from_push_message, branch_name_candidate, slug_branch_name, PushNoticeKind,
    };

    #[test]
    fn branch_candidate_slugs_human_title() {
        assert_eq!(
            branch_name_candidate("Fix popup resolve transaction read preference").as_deref(),
            Some("fix-popup-resolve-transaction-read-preference")
        );
    }

    #[test]
    fn branch_candidate_keeps_valid_branch_name() {
        assert_eq!(
            branch_name_candidate("feature/fix-popup").as_deref(),
            Some("feature/fix-popup")
        );
    }

    #[test]
    fn branch_candidate_slugs_each_path_component() {
        assert_eq!(
            branch_name_candidate("feature/Fix popup").as_deref(),
            Some("feature/fix-popup")
        );
    }

    #[test]
    fn branch_slug_rejects_punctuation_only_input() {
        assert_eq!(slug_branch_name(" : * ? "), None);
    }

    #[test]
    fn push_notice_distinguishes_publish_from_push() {
        let published = branch_from_push_message("Published feature/demo to origin").unwrap();
        assert_eq!(published.kind, PushNoticeKind::Published);
        assert_eq!(published.branch, "feature/demo");

        let pushed = branch_from_push_message("Pushed feature/demo to origin").unwrap();
        assert_eq!(pushed.kind, PushNoticeKind::Pushed);
        assert_eq!(pushed.branch, "feature/demo");
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum GitTab {
    #[default]
    Changes,
    Commits,
    PullRequests,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RemotePrimaryAction {
    Fetch,
    Pull,
    Push,
    Publish,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PushNoticeKind {
    Pushed,
    Published,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PushNoticeEvent {
    kind: PushNoticeKind,
    branch: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PullRequestLookupKey {
    repo_path: PathBuf,
    branch: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PullRequestCheckState {
    Passing,
    Pending,
    Failing,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BranchPullRequest {
    pub(crate) branch: String,
    pub(crate) number: u64,
    pub(crate) title: String,
    pub(crate) url: String,
    pub(crate) state: String,
    pub(crate) is_draft: bool,
    pub(crate) merge_state_status: Option<String>,
    pub(crate) review_decision: Option<String>,
    pub(crate) check_state: PullRequestCheckState,
}

#[derive(serde::Deserialize)]
struct GithubPullRequest {
    number: u64,
    title: String,
    url: String,
    state: String,
    #[serde(default, rename = "headRefName")]
    head_ref_name: Option<String>,
    #[serde(rename = "isDraft")]
    is_draft: bool,
    #[serde(rename = "mergeStateStatus")]
    merge_state_status: Option<String>,
    #[serde(rename = "reviewDecision")]
    review_decision: Option<String>,
    #[serde(default, rename = "statusCheckRollup")]
    status_check_rollup: Vec<serde_json::Value>,
}

#[derive(Clone)]
struct PullRequestNotice {
    message: SharedString,
    branch: String,
    url: Option<String>,
}

struct PullRequestDialog {
    notice: PullRequestNotice,
    git: Entity<GitState>,
    generation_agent: GenerationAgent,
    base_branch: String,
    base_branch_options: Vec<String>,
    base_branch_expanded: bool,
    base_branch_query: Entity<InputState>,
    ai_enabled: bool,
    generating: bool,
    error: Option<String>,
}

/// Right panel: branches, status, commit and remote actions for the active project.
pub struct GitPanel {
    workspace: Entity<Workspace>,
    git_states: Entity<GitStates>,
    agents: Entity<AgentRecords>,
    center: gpui::WeakEntity<crate::ui::center::CenterArea>,
    commit_input: Entity<InputState>,
    branch_query: Entity<InputState>,
    branches_expanded: bool,
    pub(super) collapsed_status_folders: HashSet<String>,
    pub(super) hovered_status_file: Option<(PathBuf, bool)>,
    // Branch-row hover reveals the expand chevron; PR-chip hover unfurls the
    // full pull-request title in place. Both are transient view state.
    pub(super) branch_row_hovered: bool,
    pub(super) pr_chip_hovered: bool,
    tab: GitTab,
    commit_ai_generating: bool,
    commit_ai_error: Option<String>,
    git_accounts: Vec<GitHubAccount>,
    git_accounts_loading: bool,
    git_accounts_error: Option<String>,
    last_active_project: Option<ProjectId>,
    last_active_repository: Option<PathBuf>,
    last_push_notice_message: Option<String>,
    seen_push_notice_messages: HashSet<String>,
    branch_pr_key: Option<PullRequestLookupKey>,
    branch_pr_last_message: Option<String>,
    branch_pr_checked_at: Option<Instant>,
    branch_pr_fetching: bool,
    branch_pr: Option<BranchPullRequest>,
    repo_pr_key: Option<PathBuf>,
    repo_prs_last_message: Option<String>,
    repo_prs: Vec<BranchPullRequest>,
    repo_prs_fetching: bool,
    repo_prs_checked_at: Option<Instant>,
    repo_prs_error: Option<String>,
    /// Cached `(ahead, behind)` per Solo branch for the scope flip row,
    /// refreshed in the background — never computed during render.
    pub(super) solo_ahead: HashMap<uuid::Uuid, (usize, usize)>,
    pub(super) solo_ahead_checked_at: Option<Instant>,
    pub(super) solo_ahead_fetching: bool,
    /// The Solo agent the panel is currently scoped around, if any.
    pub(super) scope_agent: Option<uuid::Uuid>,
    /// User flipped back to Main while a Solo is open. Resets to the Solo
    /// default whenever the focused Solo changes.
    pub(super) scope_main: bool,
    /// Lane GitState for the focused Solo — the substitution `active_git`
    /// serves while scoped to the lane.
    pub(super) lane_git: Option<(uuid::Uuid, Entity<GitState>)>,
}
impl GitPanel {
    /// The currently-shown git sub-view. Read by the right-panel header, which
    /// renders the Changes/Commits/PRs tabs.
    pub(crate) fn active_tab(&self) -> GitTab {
        self.tab
    }

    /// Switch the git sub-view (driven by the right-panel header tabs).
    pub(crate) fn set_active_tab(&mut self, tab: GitTab, cx: &mut Context<Self>) {
        if self.tab != tab {
            self.tab = tab;
            cx.notify();
        }
    }
}
