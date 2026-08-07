mod auth;
pub mod dto;
mod relay;
mod server;

use tokio::sync::oneshot;

use dto::{
    AgentListItemDto, AgentSnapshotDto, AnswerQuestionRequest, CommandAcceptedResponse,
    ConfigurationCatalogDto, CreateAgentRequest, DismissPlanRequest, FileDiffDto, ProjectDto,
    ResolveApprovalRequest, ResolvePlanRequest, SendMessageRequest, ShipPreviewDto, ShipRequest,
    UpdateAgentConfigurationRequest, VerificationFixRequest,
};

pub use auth::{DevicePermission, PairedDevice, PairingError, RemoteAuth};
pub use relay::RelayIdentity;
pub use server::{start_remote_server, RelayControl, RelayState};

pub type RemoteResult<T> = Result<T, RemoteError>;

#[derive(Clone, Debug)]
pub struct RemoteError {
    pub status: u16,
    pub message: String,
}

impl RemoteError {
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: 400,
            message: message.into(),
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: 404,
            message: message.into(),
        }
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: 409,
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            status: 500,
            message: message.into(),
        }
    }
}

pub enum RemoteCommand {
    GetConfiguration {
        response: oneshot::Sender<RemoteResult<ConfigurationCatalogDto>>,
    },
    ListProjects {
        response: oneshot::Sender<RemoteResult<Vec<ProjectDto>>>,
    },
    ListAgents {
        project_id: String,
        response: oneshot::Sender<RemoteResult<Vec<AgentListItemDto>>>,
    },
    GetAgent {
        agent_id: String,
        response: oneshot::Sender<RemoteResult<AgentSnapshotDto>>,
    },
    CreateAgent {
        request: CreateAgentRequest,
        response: oneshot::Sender<RemoteResult<AgentSnapshotDto>>,
    },
    SendMessage {
        agent_id: String,
        request: SendMessageRequest,
        response: oneshot::Sender<RemoteResult<CommandAcceptedResponse>>,
    },
    UpdateAgentConfiguration {
        agent_id: String,
        request: UpdateAgentConfigurationRequest,
        response: oneshot::Sender<RemoteResult<AgentSnapshotDto>>,
    },
    StopAgent {
        agent_id: String,
        response: oneshot::Sender<RemoteResult<CommandAcceptedResponse>>,
    },
    AnswerQuestion {
        agent_id: String,
        request_id: String,
        request: AnswerQuestionRequest,
        response: oneshot::Sender<RemoteResult<CommandAcceptedResponse>>,
    },
    ResolvePlan {
        agent_id: String,
        request: ResolvePlanRequest,
        response: oneshot::Sender<RemoteResult<CommandAcceptedResponse>>,
    },
    DismissPlan {
        agent_id: String,
        request: DismissPlanRequest,
        response: oneshot::Sender<RemoteResult<CommandAcceptedResponse>>,
    },
    ResolveApproval {
        agent_id: String,
        request_id: String,
        request: ResolveApprovalRequest,
        response: oneshot::Sender<RemoteResult<CommandAcceptedResponse>>,
    },
    CaptureVisualization {
        agent_id: String,
        path: String,
        response: oneshot::Sender<RemoteResult<Vec<u8>>>,
    },
    CaptureGeneratedImage {
        agent_id: String,
        path: String,
        response: oneshot::Sender<RemoteResult<Vec<u8>>>,
    },
    RequestVerificationFix {
        agent_id: String,
        request: VerificationFixRequest,
        response: oneshot::Sender<RemoteResult<CommandAcceptedResponse>>,
    },
    GetShipPreview {
        agent_id: String,
        response: oneshot::Sender<RemoteResult<ShipPreviewDto>>,
    },
    ShipAgentWork {
        agent_id: String,
        request: ShipRequest,
        response: oneshot::Sender<RemoteResult<CommandAcceptedResponse>>,
    },
    GetFileDiff {
        agent_id: String,
        path: String,
        response: oneshot::Sender<RemoteResult<FileDiffDto>>,
    },
}
