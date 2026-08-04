use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct HealthResponse {
    pub status: String,
    pub app_version: String,
    pub host_name: String,
    pub protocol_version: u32,
    pub authentication_required: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct CompletePairingRequest {
    pub code: String,
    #[serde(default)]
    pub device_name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectDto {
    pub id: String,
    pub name: String,
    pub agent_count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentListItemDto {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub backend: String,
    pub model: String,
    pub model_id: String,
    pub effort: String,
    pub access_mode: String,
    pub status: String,
    pub started_running_at: Option<u64>,
    pub last_activity_at: u64,
    pub needs_attention: bool,
    #[serde(default)]
    pub solo: bool,
    #[serde(default)]
    pub solo_branch: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConfigurationCatalogDto {
    pub providers: Vec<ProviderConfigurationDto>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderConfigurationDto {
    pub id: String,
    pub label: String,
    pub models: Vec<ModelConfigurationDto>,
    pub access_modes: Vec<AccessModeConfigurationDto>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelConfigurationDto {
    pub id: String,
    pub label: String,
    pub short_label: String,
    pub efforts: Vec<EffortConfigurationDto>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct EffortConfigurationDto {
    pub id: String,
    pub label: String,
    pub description: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccessModeConfigurationDto {
    pub id: String,
    pub label: String,
    pub description: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentSnapshotDto {
    pub agent: AgentListItemDto,
    pub project_name: String,
    pub interaction_mode: String,
    pub timeline: Vec<TimelineItemDto>,
    pub pending_user_input: Option<PendingUserInputDto>,
    pub pending_approval: Option<PendingApprovalDto>,
    pub changed_files: Vec<ChangedFileDto>,
    /// Present while a remote-initiated ship runs (or after one fails) so the
    /// phone can show progress without owning the operation.
    #[serde(default)]
    pub ship: Option<ShipStateDto>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShipStateDto {
    /// "shipping" | "failed"
    pub state: String,
    pub message: Option<String>,
    pub started_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingApprovalDto {
    pub request_id: String,
    pub kind: String,
    pub title: String,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TimelineItemDto {
    Message {
        role: String,
        text: String,
        created_at: u64,
    },
    WorkLog {
        title: String,
        detail: Option<String>,
        status: String,
        count: usize,
        updated_at: u64,
    },
    ProposedPlan {
        markdown: String,
        implemented: bool,
    },
    CodeReview {
        markdown: String,
    },
    Verification {
        id: String,
        markdown: String,
        met: usize,
        total: usize,
        /// Whether "Ask to fix" applies right now: this is the latest
        /// verification, it has unmet items, and no fix turn is in flight.
        fixable: bool,
        items: Vec<VerificationItemDto>,
    },
    ChangedFiles {
        files: Vec<ChangedFileDto>,
    },
    ShipResult {
        action: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        repository: Option<String>,
        branch: String,
        commit_sha: String,
        pr_url: Option<String>,
        pr_title: Option<String>,
        created_at: u64,
    },
    Notice {
        title: String,
        detail: Option<String>,
        created_at: u64,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerificationItemDto {
    /// "met" | "unclear" | "missed"
    pub status: String,
    pub title: String,
    pub detail: String,
    pub fix_requested: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChangedFileDto {
    pub path: String,
    pub additions: usize,
    pub deletions: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingUserInputDto {
    pub request_id: String,
    pub question_index: usize,
    pub questions: Vec<PendingQuestionDto>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingQuestionDto {
    pub id: String,
    pub header: String,
    pub question: String,
    pub options: Vec<PendingOptionDto>,
    pub multi_select: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingOptionDto {
    pub label: String,
    pub description: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct CreateAgentRequest {
    pub project_id: String,
    pub title: Option<String>,
    pub prompt: String,
    #[serde(default)]
    pub interaction_mode: InteractionModeDto,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub access_mode: Option<String>,
    /// Run in its own Solo worktree lane instead of the shared project tree.
    #[serde(default)]
    pub solo: bool,
    /// Repository root inside the opened project. Relative paths are resolved
    /// from the project folder. Required for Solo when the project contains
    /// more than one repository; optional for repository-scoped normal agents.
    #[serde(default)]
    pub repository_path: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct UpdateAgentConfigurationRequest {
    pub model: String,
    pub effort: String,
    pub access_mode: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct SendMessageRequest {
    pub text: String,
    pub client_command_id: String,
    #[serde(default)]
    pub interaction_mode: InteractionModeDto,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct AnswerQuestionRequest {
    pub answers: Vec<String>,
    pub client_command_id: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct ResolvePlanRequest {
    #[serde(default)]
    pub feedback: String,
    pub client_command_id: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct DismissPlanRequest {
    pub client_command_id: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct ResolveApprovalRequest {
    pub decision: ApprovalDecisionDto,
    pub client_command_id: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct VerificationFixRequest {
    pub verification_id: String,
    pub client_command_id: String,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ShipScopeDto {
    #[default]
    Conversation,
    All,
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct ShipRequest {
    #[serde(default)]
    pub scope: ShipScopeDto,
    #[serde(default = "default_true")]
    pub push: bool,
    #[serde(default)]
    pub open_pr: bool,
    #[serde(default)]
    pub create_branch: bool,
    #[serde(default)]
    pub pr_base_branch: Option<String>,
    pub client_command_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShipPreviewDto {
    pub branch: Option<String>,
    pub solo: bool,
    pub needs_upstream: bool,
    pub has_remote: bool,
    pub default_pr_base: String,
    pub conversation_files: Vec<String>,
    pub all_files: Vec<String>,
    pub shipping: bool,
    pub ship_error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileDiffDto {
    pub path: String,
    pub is_binary: bool,
    /// "worktree" (live uncommitted changes) | "snapshot" (captured at ship).
    pub source: String,
    pub additions: usize,
    pub deletions: usize,
    pub hunks: Vec<DiffHunkDto>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiffHunkDto {
    pub header: String,
    pub lines: Vec<DiffLineDto>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiffLineDto {
    /// "add" | "remove" | "context"
    pub origin: String,
    pub old_no: Option<u32>,
    pub new_no: Option<u32>,
    pub text: String,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecisionDto {
    Approve,
    Deny,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InteractionModeDto {
    #[default]
    Default,
    Plan,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandAcceptedResponse {
    pub accepted: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RemoteEvent {
    HostSnapshotChanged,
    ProjectChanged { project_id: String },
    AgentChanged { agent_id: String },
    AgentDeleted { agent_id: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_event_wire_format_is_stable() {
        let event = RemoteEvent::AgentChanged {
            agent_id: "a1".into(),
        };
        assert_eq!(
            serde_json::to_string(&event).unwrap(),
            r#"{"type":"agent_changed","agent_id":"a1"}"#
        );
    }

    #[test]
    fn interaction_mode_defaults_to_default() {
        let request: SendMessageRequest =
            serde_json::from_str(r#"{"text":"hello","client_command_id":"command-1"}"#).unwrap();
        assert_eq!(request.interaction_mode, InteractionModeDto::Default);
    }

    #[test]
    fn create_agent_repository_path_is_optional() {
        let request: CreateAgentRequest =
            serde_json::from_str(r#"{"project_id":"p1","prompt":"hello","solo":true}"#).unwrap();
        assert_eq!(request.repository_path, None);

        let request: CreateAgentRequest = serde_json::from_str(
            r#"{"project_id":"p1","prompt":"hello","repository_path":"frontend"}"#,
        )
        .unwrap();
        assert_eq!(request.repository_path.as_deref(), Some("frontend"));
    }

    #[test]
    fn health_response_includes_authentication_requirement() {
        let health = HealthResponse {
            status: "ok".into(),
            app_version: "1".into(),
            host_name: "Mac".into(),
            protocol_version: 2,
            authentication_required: true,
        };
        assert!(
            serde_json::to_value(health).unwrap()["authentication_required"]
                .as_bool()
                .unwrap()
        );
    }
}
