use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use async_channel::Sender;
use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{DefaultBodyLimit, Extension, Path, Query, Request, State};
use axum::http::{header, HeaderMap, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::{broadcast, oneshot};

use super::dto::{
    AcknowledgePocketCometTaskActionsRequest, AgentSyncBatchDto, AgentSyncRequest,
    AnswerQuestionRequest, CommandAcceptedResponse, CompletePairingRequest, CreateAgentRequest,
    DismissPlanRequest, HealthResponse, RemoteEvent, ResolveApprovalRequest, ResolvePlanRequest,
    SendMessageRequest, ShipRequest, SyncPocketCometTaskSourcesRequest,
    UpdateAgentConfigurationRequest, UpdateAgentStatusRequest, UpsertChoroDocumentRequest,
    VerificationFixRequest,
};
use super::{
    DevicePermission, PairedDevice, PairingError, RemoteAuth, RemoteCommand, RemoteError,
    RemoteResult,
};

// Keep Choro Remote on its own stable port. Port 3847 is used by another local
// development service on the primary workstation, while the mobile MVP and
// existing remote URL already target 3848.
const DEFAULT_PORT: u16 = 3848;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_DOCUMENT_ASSET_BYTES: usize = 25 * 1024 * 1024;
const MAX_POCKETCOMET_TASK_ASSET_BYTES: usize = 25 * 1024 * 1024;
const MAX_POCKETCOMET_TASK_SOURCE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone)]
struct ServerState {
    commands: Sender<RemoteCommand>,
    events: broadcast::Sender<RemoteEvent>,
    auth: RemoteAuth,
}

pub struct RemoteServer {
    pub address: SocketAddr,
    pub commands: async_channel::Receiver<RemoteCommand>,
    pub events: broadcast::Sender<RemoteEvent>,
    pub presence: async_channel::Receiver<usize>,
    pub relay_states: async_channel::Receiver<RelayState>,
    pub auth: RemoteAuth,
    pub relay_identity: super::RelayIdentity,
    pub relay_control: RelayControl,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum RelayState {
    Disconnected = 0,
    Connecting = 1,
    Connected = 2,
    Reconnecting = 3,
}

impl RelayState {
    fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Connecting,
            2 => Self::Connected,
            3 => Self::Reconnecting,
            _ => Self::Disconnected,
        }
    }
}

#[derive(Clone)]
pub(crate) struct RelayStateStore {
    current: Arc<AtomicU8>,
    updates: async_channel::Sender<RelayState>,
}

impl RelayStateStore {
    fn new(updates: async_channel::Sender<RelayState>) -> Self {
        Self {
            current: Arc::new(AtomicU8::new(RelayState::Disconnected as u8)),
            updates,
        }
    }

    pub(crate) fn set(&self, state: RelayState) {
        let previous = self.current.swap(state as u8, Ordering::SeqCst);
        if previous != state as u8 {
            let _ = self.updates.try_send(state);
        }
    }

    fn get(&self) -> RelayState {
        RelayState::from_u8(self.current.load(Ordering::SeqCst))
    }
}

#[derive(Clone, Copy)]
enum RelayCommand {
    Connect,
    Disconnect,
}

/// Opt-in control and observable state for the outbound relay tunnel.
///
/// Binding the loopback HTTP server is harmless (it only listens on
/// `127.0.0.1`), but the relay tunnel is what makes this Mac reachable from the
/// internet, so it must not open on its own. The tunnel stays closed until the
/// user explicitly taps Connect, which calls [`RelayControl::connect`].
#[derive(Clone)]
pub struct RelayControl {
    commands: async_channel::Sender<RelayCommand>,
    state: RelayStateStore,
}

impl RelayControl {
    fn new(commands: async_channel::Sender<RelayCommand>, state: RelayStateStore) -> Self {
        Self { commands, state }
    }

    /// Open the outbound relay tunnel. Idempotent — safe to call repeatedly.
    pub fn connect(&self) {
        let _ = self.commands.try_send(RelayCommand::Connect);
    }

    /// Close the outbound relay tunnel and stop reconnect attempts.
    pub fn disconnect(&self) {
        let _ = self.commands.try_send(RelayCommand::Disconnect);
    }

    pub fn state(&self) -> RelayState {
        self.state.get()
    }
}

pub fn start_remote_server() -> RemoteServer {
    let address = configured_address();
    let (command_tx, command_rx) = async_channel::unbounded();
    let (event_tx, _) = broadcast::channel(256);
    let (presence_tx, presence_rx) = async_channel::unbounded();
    let auth = RemoteAuth::load_default();
    let relay_identity = super::RelayIdentity::load_default();
    let (relay_command_tx, relay_command_rx) = async_channel::unbounded();
    let (relay_state_tx, relay_state_rx) = async_channel::unbounded();
    let relay_state = RelayStateStore::new(relay_state_tx);
    let relay_control = RelayControl::new(relay_command_tx, relay_state.clone());
    let state = ServerState {
        commands: command_tx,
        events: event_tx.clone(),
        auth: auth.clone(),
    };

    let thread_auth = auth.clone();
    let thread_events = event_tx.clone();
    let thread_relay_identity = relay_identity.clone();
    thread::Builder::new()
        .name("choro-remote-server".into())
        .spawn(move || {
            // The remote bridge only performs lightweight async socket I/O and
            // forwards commands to the GPUI thread. A current-thread runtime
            // avoids keeping extra worker threads alive in the normal desktop
            // app while preserving concurrent HTTP and WebSocket handling.
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    eprintln!("failed to create Choro remote runtime: {error}");
                    return;
                }
            };
            runtime.block_on(async move {
                let listener = match tokio::net::TcpListener::bind(address).await {
                    Ok(listener) => listener,
                    Err(error) => {
                        eprintln!("failed to bind Choro remote server at {address}: {error}");
                        return;
                    }
                };
                if let Some(relay_url) = super::relay::configured_url() {
                    // Opt-in: the outbound relay tunnel stays closed until the
                    // user taps Connect. The loopback listener above is bound
                    // regardless (it is reachable only from this machine), but
                    // the relay is what exposes the Mac to paired phones, so it
                    // must never auto-start.
                    let supervisor_state = relay_state.clone();
                    tokio::spawn(async move {
                        let mut relay_task: Option<tokio::task::JoinHandle<()>> = None;
                        while let Ok(command) = relay_command_rx.recv().await {
                            if relay_task.as_ref().is_some_and(|task| task.is_finished()) {
                                if let Some(task) = relay_task.take() {
                                    let _ = task.await;
                                }
                            }
                            match command {
                                RelayCommand::Connect if relay_task.is_none() => {
                                    let task_state = supervisor_state.clone();
                                    let task_url = relay_url.clone();
                                    let task_identity = thread_relay_identity.clone();
                                    let task_auth = thread_auth.clone();
                                    let task_events = thread_events.clone();
                                    let task_presence = presence_tx.clone();
                                    relay_task = Some(tokio::spawn(async move {
                                        super::relay::run(
                                            task_url,
                                            task_identity,
                                            address,
                                            task_auth,
                                            task_events,
                                            task_presence,
                                            task_state,
                                        )
                                        .await;
                                    }));
                                }
                                RelayCommand::Disconnect => {
                                    if let Some(task) = relay_task.take() {
                                        task.abort();
                                        let _ = task.await;
                                    }
                                    let _ = presence_tx.send(0).await;
                                    supervisor_state.set(RelayState::Disconnected);
                                }
                                RelayCommand::Connect => {}
                            }
                        }
                    });
                } else {
                    relay_state.set(RelayState::Disconnected);
                }
                eprintln!("Choro remote control listening on http://{address}");
                if let Err(error) = axum::serve(listener, router(state)).await {
                    eprintln!("Choro remote server stopped: {error}");
                }
            });
        })
        .expect("failed to start Choro remote server thread");

    RemoteServer {
        address,
        commands: command_rx,
        events: event_tx,
        presence: presence_rx,
        relay_states: relay_state_rx,
        auth,
        relay_identity,
        relay_control,
    }
}

fn configured_address() -> SocketAddr {
    std::env::var("CHORO_REMOTE_ADDR")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            DEFAULT_PORT,
        ))
}

fn router(state: ServerState) -> Router {
    let protected = Router::new()
        .route(
            "/v1/device",
            get(current_device).delete(revoke_current_device),
        )
        .route("/v1/configuration", get(configuration))
        .route("/v1/projects", get(list_projects))
        .route(
            "/v1/projects/{project_id}/documents/{document_id}",
            get(get_document).put(upsert_document),
        )
        .route(
            "/v1/projects/{project_id}/documents/{document_id}/assets",
            post(store_document_asset).layer(DefaultBodyLimit::max(MAX_DOCUMENT_ASSET_BYTES)),
        )
        .route(
            "/v1/projects/{project_id}/task-sources/pocketcomet/tasks/{task_id}/assets/{attachment_id}",
            post(store_pocketcomet_task_asset)
                .layer(DefaultBodyLimit::max(MAX_POCKETCOMET_TASK_ASSET_BYTES)),
        )
        .route(
            "/v1/task-sources/pocketcomet/sync",
            post(sync_pocketcomet_task_sources)
                .layer(DefaultBodyLimit::max(MAX_POCKETCOMET_TASK_SOURCE_BYTES)),
        )
        .route(
            "/v1/task-sources/pocketcomet/actions/acknowledge",
            post(acknowledge_pocketcomet_task_actions),
        )
        .route("/v1/projects/{project_id}/agents", get(list_agents))
        .route("/v1/agents/sync", post(sync_agents))
        .route("/v1/agents/{agent_id}", get(get_agent))
        .route("/v1/agents", post(create_agent))
        .route("/v1/agents/{agent_id}/open", post(open_agent))
        .route("/v1/agents/{agent_id}/status", post(update_agent_status))
        .route(
            "/v1/agents/{agent_id}/completed-turns",
            get(completed_turns),
        )
        .route("/v1/agents/{agent_id}/messages", post(send_message))
        .route(
            "/v1/agents/{agent_id}/configuration",
            post(update_agent_configuration),
        )
        .route("/v1/agents/{agent_id}/stop", post(stop_agent))
        .route(
            "/v1/agents/{agent_id}/questions/{request_id}/answer",
            post(answer_question),
        )
        .route("/v1/agents/{agent_id}/plan/resolve", post(resolve_plan))
        .route("/v1/agents/{agent_id}/plan/dismiss", post(dismiss_plan))
        .route(
            "/v1/agents/{agent_id}/approvals/{request_id}/resolve",
            post(resolve_approval),
        )
        .route(
            "/v1/agents/{agent_id}/verification/fix",
            post(verification_fix),
        )
        .route("/v1/agents/{agent_id}/ship/preview", get(ship_preview))
        .route("/v1/agents/{agent_id}/ship", post(ship_agent_work))
        .route("/v1/agents/{agent_id}/diff", get(file_diff))
        .route(
            "/v1/agents/{agent_id}/visualizations/snapshot",
            get(visualization_snapshot),
        )
        .route(
            "/v1/agents/{agent_id}/images/preview",
            get(generated_image_preview),
        )
        .route("/v1/events", get(events))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_authentication,
        ));

    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/pairing/status", get(pairing_status))
        .route("/v1/pairing/complete", post(complete_pairing))
        .merge(protected)
        .with_state(state)
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".into(),
        app_version: env!("CARGO_PKG_VERSION").into(),
        host_name: std::env::var("HOSTNAME").unwrap_or_else(|_| "Choro Mac".into()),
        protocol_version: 10,
        authentication_required: true,
    })
}

async fn pairing_status(State(state): State<ServerState>) -> impl IntoResponse {
    Json(state.auth.public_status())
}

async fn complete_pairing(
    State(state): State<ServerState>,
    Json(request): Json<CompletePairingRequest>,
) -> Response {
    match state.auth.pair(&request.code, &request.device_name) {
        Ok(result) => Json(result).into_response(),
        Err(PairingError::NotActive) => (
            StatusCode::CONFLICT,
            Json(json!({ "error": "Start pairing from Choro Desktop first" })),
        )
            .into_response(),
        Err(PairingError::Expired) => (
            StatusCode::CONFLICT,
            Json(json!({ "error": "Pairing code expired; start a new pairing session" })),
        )
            .into_response(),
        Err(PairingError::InvalidCode) => (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Pairing code is incorrect" })),
        )
            .into_response(),
        Err(PairingError::TooManyAttempts) => (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({ "error": "Too many attempts; start pairing again on the Mac" })),
        )
            .into_response(),
        Err(PairingError::DeviceLimit) => (
            StatusCode::CONFLICT,
            Json(json!({ "error": "Revoke an old device before pairing another one" })),
        )
            .into_response(),
        Err(PairingError::Storage(error)) => api_error(RemoteError::internal(format!(
            "Could not save paired device: {error}"
        ))),
    }
}

async fn require_authentication(
    State(state): State<ServerState>,
    mut request: Request,
    next: Next,
) -> Response {
    let token = request_token(request.headers());
    if let Some(device) = token
        .as_deref()
        .and_then(|token| state.auth.authorize_device(token, None))
    {
        if request.method() != Method::GET
            && request.uri().path() != "/v1/device"
            && request.uri().path() != "/v1/agents/sync"
            && !may_mutate(device.permission)
        {
            return permission_denied();
        }
        request.extensions_mut().insert(device);
        return next.run(request).await;
    }
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "error": "This device is not paired with Choro Desktop" })),
    )
        .into_response()
}

async fn current_device(Extension(device): Extension<PairedDevice>) -> Json<PairedDevice> {
    Json(device)
}

async fn revoke_current_device(
    State(state): State<ServerState>,
    Extension(device): Extension<PairedDevice>,
) -> Response {
    if let Err(error) = command_result(&state, |response| {
        RemoteCommand::RemovePocketCometTaskSources {
            device_id: device.id.clone(),
            response,
        }
    })
    .await
    {
        return api_error(error);
    }
    match state.auth.revoke(&device.id) {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => api_error(RemoteError::not_found("paired device not found")),
        Err(error) => api_error(RemoteError::internal(format!(
            "Could not revoke paired device: {error:?}"
        ))),
    }
}

fn request_token(headers: &HeaderMap) -> Option<String> {
    if let Some(token) = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(token.to_string());
    }
    headers
        .get(header::SEC_WEBSOCKET_PROTOCOL)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            value
                .split(',')
                .map(str::trim)
                .find_map(|protocol| protocol.strip_prefix("choro-auth."))
        })
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

async fn list_projects(State(state): State<ServerState>) -> Response {
    command(&state, |response| RemoteCommand::ListProjects { response }).await
}

async fn get_document(
    State(state): State<ServerState>,
    Path((project_id, document_id)): Path<(String, String)>,
) -> Response {
    command(&state, |response| RemoteCommand::GetDocument {
        project_id,
        document_id,
        response,
    })
    .await
}

async fn upsert_document(
    State(state): State<ServerState>,
    Extension(device): Extension<PairedDevice>,
    Path((project_id, document_id)): Path<(String, String)>,
    Json(request): Json<UpsertChoroDocumentRequest>,
) -> Response {
    if device.permission != DevicePermission::FullAccess {
        return permission_denied();
    }
    command(&state, |response| RemoteCommand::UpsertDocument {
        project_id,
        document_id,
        request,
        response,
    })
    .await
}

async fn store_document_asset(
    State(state): State<ServerState>,
    Extension(device): Extension<PairedDevice>,
    Path((project_id, document_id)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if device.permission != DevicePermission::FullAccess {
        return permission_denied();
    }
    if body.is_empty() || body.len() > MAX_DOCUMENT_ASSET_BYTES {
        return api_error(RemoteError::bad_request(
            "Document assets must be between 1 byte and 25 MB",
        ));
    }
    let mime = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("application/octet-stream")
        .to_string();
    command(&state, |response| RemoteCommand::StoreDocumentAsset {
        project_id,
        document_id,
        mime,
        bytes: body.to_vec(),
        response,
    })
    .await
}

async fn store_pocketcomet_task_asset(
    State(state): State<ServerState>,
    Extension(device): Extension<PairedDevice>,
    Path((project_id, task_id, attachment_id)): Path<(String, String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if device.permission != DevicePermission::FullAccess {
        return full_access_required();
    }
    if body.is_empty() || body.len() > MAX_POCKETCOMET_TASK_ASSET_BYTES {
        return api_error(RemoteError::bad_request(
            "Task images must be between 1 byte and 25 MB",
        ));
    }
    let mime = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .filter(|value| value.to_ascii_lowercase().starts_with("image/"));
    let Some(mime) = mime else {
        return api_error(RemoteError::bad_request(
            "PocketComet task attachments must be images",
        ));
    };
    command(&state, |response| {
        RemoteCommand::StorePocketCometTaskAsset {
            project_id,
            task_id,
            attachment_id,
            mime: mime.to_string(),
            bytes: body.to_vec(),
            response,
        }
    })
    .await
}

async fn sync_pocketcomet_task_sources(
    State(state): State<ServerState>,
    Extension(device): Extension<PairedDevice>,
    Json(request): Json<SyncPocketCometTaskSourcesRequest>,
) -> Response {
    if device.permission != DevicePermission::FullAccess {
        return full_access_required();
    }
    command(&state, |response| {
        RemoteCommand::SyncPocketCometTaskSources {
            device_id: device.id,
            request,
            response,
        }
    })
    .await
}

async fn acknowledge_pocketcomet_task_actions(
    State(state): State<ServerState>,
    Extension(device): Extension<PairedDevice>,
    Json(request): Json<AcknowledgePocketCometTaskActionsRequest>,
) -> Response {
    if device.permission != DevicePermission::FullAccess {
        return full_access_required();
    }
    command(&state, |response| {
        RemoteCommand::AcknowledgePocketCometTaskActions {
            device_id: device.id,
            request,
            response,
        }
    })
    .await
}

async fn configuration(State(state): State<ServerState>) -> Response {
    command(&state, |response| RemoteCommand::GetConfiguration {
        response,
    })
    .await
}

async fn list_agents(State(state): State<ServerState>, Path(project_id): Path<String>) -> Response {
    command(&state, |response| RemoteCommand::ListAgents {
        project_id,
        response,
    })
    .await
}

async fn get_agent(State(state): State<ServerState>, Path(agent_id): Path<String>) -> Response {
    command(&state, |response| RemoteCommand::GetAgent {
        agent_id,
        response,
    })
    .await
}

async fn sync_agents(
    State(state): State<ServerState>,
    Extension(device): Extension<PairedDevice>,
    Json(request): Json<AgentSyncRequest>,
) -> Response {
    if request.agent_ids.len() > 250 {
        return api_error(RemoteError::bad_request(
            "agent sync accepts at most 250 identities",
        ));
    }
    let (response_tx, response_rx) = oneshot::channel();
    if state
        .commands
        .send(RemoteCommand::SyncAgents {
            agent_ids: request.agent_ids,
            response: response_tx,
        })
        .await
        .is_err()
    {
        return api_error(RemoteError::internal(
            "desktop command bridge is unavailable",
        ));
    }
    match tokio::time::timeout(COMMAND_TIMEOUT, response_rx).await {
        Ok(Ok(Ok(agents))) => Json(AgentSyncBatchDto {
            device_id: device.id,
            agents,
        })
        .into_response(),
        Ok(Ok(Err(error))) => api_error(error),
        Ok(Err(_)) => api_error(RemoteError::internal("desktop command was cancelled")),
        Err(_) => api_error(RemoteError::internal("desktop command timed out")),
    }
}

async fn create_agent(
    State(state): State<ServerState>,
    Extension(device): Extension<PairedDevice>,
    Json(request): Json<CreateAgentRequest>,
) -> Response {
    if !may_set_access_mode(device.permission, request.access_mode.as_deref()) {
        return full_access_required();
    }
    command(&state, |response| RemoteCommand::CreateAgent {
        request,
        response,
    })
    .await
}

async fn open_agent(State(state): State<ServerState>, Path(agent_id): Path<String>) -> Response {
    command(&state, |response| RemoteCommand::OpenAgent {
        agent_id,
        response,
    })
    .await
}

async fn update_agent_status(
    State(state): State<ServerState>,
    Path(agent_id): Path<String>,
    Json(request): Json<UpdateAgentStatusRequest>,
) -> Response {
    command(&state, |response| RemoteCommand::UpdateAgentStatus {
        agent_id,
        request,
        response,
    })
    .await
}

#[derive(Deserialize)]
struct CompletedTurnsQuery {
    #[serde(default)]
    after_sequence: i64,
}

async fn completed_turns(
    State(state): State<ServerState>,
    Path(agent_id): Path<String>,
    Query(query): Query<CompletedTurnsQuery>,
) -> Response {
    command(&state, |response| RemoteCommand::CompletedTurns {
        agent_id,
        after_sequence: query.after_sequence.max(-1),
        response,
    })
    .await
}

async fn send_message(
    State(state): State<ServerState>,
    Path(agent_id): Path<String>,
    Json(request): Json<SendMessageRequest>,
) -> Response {
    command(&state, |response| RemoteCommand::SendMessage {
        agent_id,
        request,
        response,
    })
    .await
}

async fn update_agent_configuration(
    State(state): State<ServerState>,
    Path(agent_id): Path<String>,
    Extension(device): Extension<PairedDevice>,
    Json(request): Json<UpdateAgentConfigurationRequest>,
) -> Response {
    if !may_set_access_mode(device.permission, Some(&request.access_mode)) {
        return full_access_required();
    }
    command(&state, |response| RemoteCommand::UpdateAgentConfiguration {
        agent_id,
        request,
        response,
    })
    .await
}

async fn stop_agent(State(state): State<ServerState>, Path(agent_id): Path<String>) -> Response {
    command(&state, |response| RemoteCommand::StopAgent {
        agent_id,
        response,
    })
    .await
}

async fn answer_question(
    State(state): State<ServerState>,
    Path((agent_id, request_id)): Path<(String, String)>,
    Json(request): Json<AnswerQuestionRequest>,
) -> Response {
    command(&state, |response| RemoteCommand::AnswerQuestion {
        agent_id,
        request_id,
        request,
        response,
    })
    .await
}

async fn resolve_plan(
    State(state): State<ServerState>,
    Path(agent_id): Path<String>,
    Json(request): Json<ResolvePlanRequest>,
) -> Response {
    command(&state, |response| RemoteCommand::ResolvePlan {
        agent_id,
        request,
        response,
    })
    .await
}

async fn dismiss_plan(
    State(state): State<ServerState>,
    Path(agent_id): Path<String>,
    Json(request): Json<DismissPlanRequest>,
) -> Response {
    command(&state, |response| RemoteCommand::DismissPlan {
        agent_id,
        request,
        response,
    })
    .await
}

async fn resolve_approval(
    State(state): State<ServerState>,
    Path((agent_id, request_id)): Path<(String, String)>,
    Extension(device): Extension<PairedDevice>,
    Json(request): Json<ResolveApprovalRequest>,
) -> Response {
    if device.permission != DevicePermission::FullAccess {
        return full_access_required();
    }
    command(&state, |response| RemoteCommand::ResolveApproval {
        agent_id,
        request_id,
        request,
        response,
    })
    .await
}

async fn verification_fix(
    State(state): State<ServerState>,
    Path(agent_id): Path<String>,
    Json(request): Json<VerificationFixRequest>,
) -> Response {
    command(&state, |response| RemoteCommand::RequestVerificationFix {
        agent_id,
        request,
        response,
    })
    .await
}

async fn ship_preview(State(state): State<ServerState>, Path(agent_id): Path<String>) -> Response {
    command(&state, |response| RemoteCommand::GetShipPreview {
        agent_id,
        response,
    })
    .await
}

async fn ship_agent_work(
    State(state): State<ServerState>,
    Path(agent_id): Path<String>,
    Extension(device): Extension<PairedDevice>,
    Json(request): Json<ShipRequest>,
) -> Response {
    // Shipping commits, pushes, and opens PRs — the same blast radius as
    // approving arbitrary commands, so it needs the same device tier.
    if device.permission != DevicePermission::FullAccess {
        return full_access_required();
    }
    command(&state, |response| RemoteCommand::ShipAgentWork {
        agent_id,
        request,
        response,
    })
    .await
}

#[derive(Deserialize)]
struct FileDiffQuery {
    path: String,
}

async fn file_diff(
    State(state): State<ServerState>,
    Path(agent_id): Path<String>,
    Query(query): Query<FileDiffQuery>,
) -> Response {
    command(&state, |response| RemoteCommand::GetFileDiff {
        agent_id,
        path: query.path,
        response,
    })
    .await
}

#[derive(Deserialize)]
struct VisualizationSnapshotQuery {
    path: String,
}

async fn visualization_snapshot(
    State(state): State<ServerState>,
    Path(agent_id): Path<String>,
    Query(query): Query<VisualizationSnapshotQuery>,
) -> Response {
    let (response_tx, response_rx) = oneshot::channel();
    if state
        .commands
        .send(RemoteCommand::CaptureVisualization {
            agent_id,
            path: query.path,
            response: response_tx,
        })
        .await
        .is_err()
    {
        return api_error(RemoteError::internal(
            "desktop command bridge is unavailable",
        ));
    }
    match tokio::time::timeout(COMMAND_TIMEOUT, response_rx).await {
        Ok(Ok(Ok(png))) => (
            [
                (header::CONTENT_TYPE, "image/png"),
                (header::CACHE_CONTROL, "no-store"),
            ],
            png,
        )
            .into_response(),
        Ok(Ok(Err(error))) => api_error(error),
        Ok(Err(_)) => api_error(RemoteError::internal("desktop capture was cancelled")),
        Err(_) => api_error(RemoteError::internal("desktop capture timed out")),
    }
}

async fn generated_image_preview(
    State(state): State<ServerState>,
    Path(agent_id): Path<String>,
    Query(query): Query<VisualizationSnapshotQuery>,
) -> Response {
    let (response_tx, response_rx) = oneshot::channel();
    if state
        .commands
        .send(RemoteCommand::CaptureGeneratedImage {
            agent_id,
            path: query.path,
            response: response_tx,
        })
        .await
        .is_err()
    {
        return api_error(RemoteError::internal(
            "desktop command bridge is unavailable",
        ));
    }
    match tokio::time::timeout(COMMAND_TIMEOUT, response_rx).await {
        Ok(Ok(Ok(jpeg))) => (
            [
                (header::CONTENT_TYPE, "image/jpeg"),
                (header::CACHE_CONTROL, "private, max-age=300"),
            ],
            jpeg,
        )
            .into_response(),
        Ok(Ok(Err(error))) => api_error(error),
        Ok(Err(_)) => api_error(RemoteError::internal("image preview was cancelled")),
        Err(_) => api_error(RemoteError::internal("image preview timed out")),
    }
}

async fn events(State(state): State<ServerState>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.protocols(["choro-v1"])
        .on_upgrade(move |socket| event_stream(socket, state.events.subscribe()))
}

async fn event_stream(mut socket: WebSocket, mut events: broadcast::Receiver<RemoteEvent>) {
    while let Ok(event) = events.recv().await {
        let Ok(payload) = serde_json::to_string(&event) else {
            continue;
        };
        if socket.send(Message::Text(payload.into())).await.is_err() {
            break;
        }
    }
}

async fn command<T, F>(state: &ServerState, make_command: F) -> Response
where
    T: Serialize,
    F: FnOnce(oneshot::Sender<RemoteResult<T>>) -> RemoteCommand,
{
    match command_result(state, make_command).await {
        Ok(value) => Json(value).into_response(),
        Err(error) => api_error(error),
    }
}

async fn command_result<T, F>(state: &ServerState, make_command: F) -> RemoteResult<T>
where
    F: FnOnce(oneshot::Sender<RemoteResult<T>>) -> RemoteCommand,
{
    let (response_tx, response_rx) = oneshot::channel();
    if state
        .commands
        .send(make_command(response_tx))
        .await
        .is_err()
    {
        return Err(RemoteError::internal(
            "desktop command bridge is unavailable",
        ));
    }
    match tokio::time::timeout(COMMAND_TIMEOUT, response_rx).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => Err(RemoteError::internal("desktop command was cancelled")),
        Err(_) => Err(RemoteError::internal("desktop command timed out")),
    }
}

fn api_error(error: RemoteError) -> Response {
    let status = StatusCode::from_u16(error.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (status, Json(json!({ "error": error.message }))).into_response()
}

fn may_mutate(permission: DevicePermission) -> bool {
    permission != DevicePermission::ViewOnly
}

fn may_set_access_mode(permission: DevicePermission, access_mode: Option<&str>) -> bool {
    may_mutate(permission)
        && (access_mode != Some("full_access") || permission == DevicePermission::FullAccess)
}

fn permission_denied() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(json!({ "error": "This device is not allowed to perform that action" })),
    )
        .into_response()
}

fn full_access_required() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(json!({
            "error": "This action requires Full access for this device. Change its permission in Desktop Settings → Remote access."
        })),
    )
        .into_response()
}

#[allow(dead_code)]
fn _assert_command_response_is_serializable(_: CommandAcceptedResponse) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_state_is_observable_and_deduplicated() {
        let (updates_tx, updates_rx) = async_channel::unbounded();
        let state = RelayStateStore::new(updates_tx);
        assert_eq!(state.get(), RelayState::Disconnected);

        state.set(RelayState::Connecting);
        assert_eq!(state.get(), RelayState::Connecting);
        assert_eq!(updates_rx.try_recv().unwrap(), RelayState::Connecting);

        state.set(RelayState::Connecting);
        assert!(updates_rx.try_recv().is_err());
        state.set(RelayState::Connected);
        assert_eq!(updates_rx.try_recv().unwrap(), RelayState::Connected);
    }

    #[test]
    fn relay_control_sends_connect_and_disconnect_commands() {
        let (commands_tx, commands_rx) = async_channel::unbounded();
        let (updates_tx, _updates_rx) = async_channel::unbounded();
        let control = RelayControl::new(commands_tx, RelayStateStore::new(updates_tx));

        control.connect();
        assert!(matches!(commands_rx.try_recv(), Ok(RelayCommand::Connect)));
        control.disconnect();
        assert!(matches!(
            commands_rx.try_recv(),
            Ok(RelayCommand::Disconnect)
        ));
    }

    #[test]
    fn extracts_http_bearer_token() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            "Bearer secret-token".parse().unwrap(),
        );
        assert_eq!(request_token(&headers).as_deref(), Some("secret-token"));
    }

    #[test]
    fn extracts_websocket_subprotocol_token() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::SEC_WEBSOCKET_PROTOCOL,
            "choro-v1, choro-auth.secret-token".parse().unwrap(),
        );
        assert_eq!(request_token(&headers).as_deref(), Some("secret-token"));
    }

    #[test]
    fn desktop_api_rechecks_device_permissions() {
        assert!(!may_mutate(DevicePermission::ViewOnly));
        assert!(may_mutate(DevicePermission::Control));
        assert!(may_set_access_mode(
            DevicePermission::Control,
            Some("workspace_write")
        ));
        assert!(!may_set_access_mode(
            DevicePermission::Control,
            Some("full_access")
        ));
        assert!(may_set_access_mode(
            DevicePermission::FullAccess,
            Some("full_access")
        ));
    }
}
