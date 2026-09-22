use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write as _;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use async_channel::Sender;
use base64::engine::general_purpose::{STANDARD as BASE64, URL_SAFE_NO_PAD};
use base64::Engine as _;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use ed25519_dalek::{Signer as _, SigningKey};
use futures_util::{SinkExt, StreamExt};
use hkdf::Hkdf;
use rand::rngs::OsRng;
use rand::RngCore as _;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::sync::broadcast;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::SEC_WEBSOCKET_PROTOCOL;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;
use x25519_dalek::{PublicKey, StaticSecret};

use super::dto::RemoteEvent;
use super::server::RelayStateStore;
use super::{DevicePermission, PairingError, RelayState, RemoteAuth};

const PROTOCOL: u8 = 2;
const RELAY_PROTOCOL: u32 = 3;
const HOST_AUTH_DOMAIN: &[u8] = b"choro-relay-host-v3\0";
const HOST_AUTH_TIMEOUT: Duration = Duration::from_secs(5);
const RECONNECT_MIN: Duration = Duration::from_secs(1);
const RECONNECT_MAX: Duration = Duration::from_secs(20);
const MAX_PATH_BYTES: usize = 2_048;
const MAX_BODY_BYTES: usize = 6 * 1024 * 1024;
const SESSION_TTL_SECS: u64 = 30 * 60;
const AUTH_SYNC_INTERVAL: Duration = Duration::from_secs(1);
const DEFAULT_RELAY_URL: &str = "https://choro-relay.onrender.com";
const RELAY_KEYCHAIN_SERVICE: &str = "com.ritmus.choro.remote.relay";
const RELAY_KEYCHAIN_ACCOUNT: &str = "host-identity";

fn relay_keychain_service() -> String {
    std::env::var("CHORO_RELAY_KEYCHAIN_SERVICE")
        .ok()
        .filter(|service| !service.trim().is_empty())
        .unwrap_or_else(|| RELAY_KEYCHAIN_SERVICE.into())
}

#[derive(Clone)]
pub struct RelayIdentity {
    signing_key: [u8; 32],
    room_id: String,
}

#[derive(Serialize, Deserialize)]
struct StoredRelayIdentity {
    #[serde(default, alias = "host_secret")]
    signing_key: Option<String>,
}

#[derive(Deserialize)]
struct RelayIncoming {
    #[serde(rename = "type")]
    kind: String,
    client_id: Option<String>,
    payload: Option<String>,
    protocol: Option<u32>,
    nonce: Option<String>,
}

#[derive(Serialize)]
struct RelayOutgoing<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    client_id: &'a str,
    payload: String,
}

#[derive(Serialize, Deserialize)]
struct SealedEnvelope {
    version: u8,
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    device_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sequence: Option<u64>,
    nonce: String,
    ciphertext: String,
}

#[derive(Deserialize)]
struct PairRequest {
    request_id: String,
    code: String,
    #[serde(default)]
    device_name: String,
}

#[derive(Deserialize)]
struct TunnelRequest {
    request_id: String,
    method: String,
    path: String,
    body: Option<String>,
}

#[derive(Deserialize)]
struct SessionHello {
    session_id: String,
    device_id: String,
    token: String,
    client_public_key: String,
    client_nonce: String,
}

#[derive(Serialize)]
struct SessionReady {
    session_id: String,
    server_public_key: String,
    server_nonce: String,
    expires_at: u64,
}

struct Session {
    session_id: String,
    device_id: String,
    token: String,
    receive_key: [u8; 32],
    send_key: [u8; 32],
    receive_sequence: u64,
    send_sequence: u64,
    expires_at: u64,
}

#[derive(Serialize)]
struct RelayAuthSync {
    #[serde(rename = "type")]
    kind: &'static str,
    admission_hashes: Vec<String>,
    pairing_open: bool,
}

#[derive(Serialize)]
struct TunnelResponse {
    request_id: String,
    status: u16,
    content_type: String,
    body_base64: String,
}

impl RelayIdentity {
    pub fn load_default() -> Self {
        let path = ide_core::local_store::LocalStore::open_default()
            .map(|store| {
                store
                    .app_data_dir()
                    .join("remote")
                    .join("relay-identity.json")
            })
            .unwrap_or_else(|_| PathBuf::from("relay-identity.json"));
        // An explicitly offline instance has no reason to access the Keychain.
        // Use the existing private-file fallback for its isolated local identity.
        Self::load_or_create(
            &path,
            cfg!(target_os = "macos") && configured_url().is_some(),
        )
    }

    fn load_or_create(path: &Path, use_keychain: bool) -> Self {
        let file_key = fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<StoredRelayIdentity>(&bytes).ok())
            .and_then(|value| value.signing_key)
            .filter(|value| decode_key(value).is_some());
        let keychain_key = use_keychain
            .then(relay_keychain_get)
            .flatten()
            .filter(|value| decode_key(value).is_some());
        let mut stored_in_keychain = keychain_key.is_some();
        let encoded_key = keychain_key.or(file_key).unwrap_or_else(|| {
            let mut key = [0_u8; 32];
            OsRng.fill_bytes(&mut key);
            URL_SAFE_NO_PAD.encode(key)
        });
        if use_keychain && !stored_in_keychain && relay_keychain_set(&encoded_key).is_ok() {
            stored_in_keychain = true;
        }
        let file = StoredRelayIdentity {
            signing_key: (!stored_in_keychain).then_some(encoded_key.clone()),
        };
        if let Ok(bytes) = serde_json::to_vec_pretty(&file) {
            if let Err(error) = write_private_atomic(path, &bytes) {
                eprintln!("failed to persist Choro relay identity metadata: {error}");
            }
        }
        let signing_key = decode_key(&encoded_key).expect("validated relay signing key");
        let room_id = URL_SAFE_NO_PAD.encode(SigningKey::from_bytes(&signing_key).verifying_key());
        Self {
            signing_key,
            room_id,
        }
    }

    pub fn room_id(&self) -> &str {
        &self.room_id
    }

    fn sign_challenge(&self, nonce: &[u8; 32]) -> String {
        let message = host_auth_message(&self.room_id, nonce);
        let signature = SigningKey::from_bytes(&self.signing_key).sign(&message);
        URL_SAFE_NO_PAD.encode(signature.to_bytes())
    }
}

impl std::fmt::Debug for RelayIdentity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RelayIdentity")
            .field("room_id", &self.room_id)
            .finish_non_exhaustive()
    }
}

fn host_auth_message(room_id: &str, nonce: &[u8; 32]) -> Vec<u8> {
    let mut message = Vec::with_capacity(HOST_AUTH_DOMAIN.len() + room_id.len() + 1 + nonce.len());
    message.extend_from_slice(HOST_AUTH_DOMAIN);
    message.extend_from_slice(room_id.as_bytes());
    message.push(0);
    message.extend_from_slice(nonce);
    message
}

pub fn configured_url() -> Option<String> {
    std::env::var("CHORO_RELAY_URL")
        .ok()
        .or_else(|| option_env!("CHORO_RELAY_URL").map(str::to_string))
        .or_else(|| Some(DEFAULT_RELAY_URL.to_string()))
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| value.starts_with("https://") || value.starts_with("http://"))
}

pub async fn run(
    relay_url: String,
    identity: RelayIdentity,
    local_address: SocketAddr,
    auth: RemoteAuth,
    events: broadcast::Sender<RemoteEvent>,
    presence: Sender<usize>,
    state: RelayStateStore,
) {
    let mut backoff = RECONNECT_MIN;
    let mut first_attempt = true;
    loop {
        state.set(if first_attempt {
            RelayState::Connecting
        } else {
            RelayState::Reconnecting
        });
        match run_connection(
            &relay_url,
            &identity,
            local_address,
            auth.clone(),
            events.subscribe(),
            &presence,
            &state,
        )
        .await
        {
            Ok(()) => {
                eprintln!("Choro relay connection closed; reconnecting");
                backoff = RECONNECT_MIN;
            }
            Err(error) => eprintln!("Choro relay unavailable: {error}"),
        }
        first_attempt = false;
        state.set(RelayState::Reconnecting);
        let _ = presence.send(0).await;
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(RECONNECT_MAX);
    }
}

async fn run_connection(
    relay_url: &str,
    identity: &RelayIdentity,
    local_address: SocketAddr,
    auth: RemoteAuth,
    mut events: broadcast::Receiver<RemoteEvent>,
    presence: &Sender<usize>,
    state: &RelayStateStore,
) -> Result<(), String> {
    let websocket_base = relay_url
        .replacen("https://", "wss://", 1)
        .replacen("http://", "ws://", 1);
    let url = format!(
        "{websocket_base}/v1/relay?role=host&room={}",
        identity.room_id
    );
    let mut request = url
        .into_client_request()
        .map_err(|error| error.to_string())?;
    // Only the public protocol identifier is present in the upgrade headers.
    // Host ownership is proven after upgrade with a nonce-bound signature.
    request.headers_mut().insert(
        SEC_WEBSOCKET_PROTOCOL,
        "choro-relay-v1".parse().map_err(
            |error: tokio_tungstenite::tungstenite::http::header::InvalidHeaderValue| {
                error.to_string()
            },
        )?,
    );
    let (socket, _) = connect_async(request)
        .await
        .map_err(|error| error.to_string())?;
    let (mut sink, mut stream) = socket.split();

    let challenge = tokio::time::timeout(HOST_AUTH_TIMEOUT, stream.next())
        .await
        .map_err(|_| "relay host challenge timed out".to_string())?
        .ok_or_else(|| "relay closed before host authentication".to_string())?
        .map_err(|error| error.to_string())?;
    let Message::Text(challenge) = challenge else {
        return Err("relay sent an invalid host challenge".into());
    };
    let challenge: RelayIncoming = serde_json::from_str(challenge.as_ref())
        .map_err(|_| "relay sent an invalid host challenge")?;
    if challenge.kind != "challenge" || challenge.protocol != Some(RELAY_PROTOCOL) {
        return Err("relay host protocol is incompatible".into());
    }
    let nonce = challenge
        .nonce
        .as_deref()
        .and_then(decode_key)
        .ok_or_else(|| "relay sent an invalid host challenge nonce".to_string())?;
    let host_auth = serde_json::to_string(&json!({
        "type": "host_auth",
        "nonce": URL_SAFE_NO_PAD.encode(nonce),
        "signature": identity.sign_challenge(&nonce),
    }))
    .map_err(|error| error.to_string())?;
    sink.send(Message::Text(host_auth.into()))
        .await
        .map_err(|error| error.to_string())?;
    let ready = tokio::time::timeout(HOST_AUTH_TIMEOUT, stream.next())
        .await
        .map_err(|_| "relay host authentication timed out".to_string())?
        .ok_or_else(|| "relay closed during host authentication".to_string())?
        .map_err(|error| error.to_string())?;
    let Message::Text(ready) = ready else {
        return Err("relay sent an invalid host authentication response".into());
    };
    let ready: RelayIncoming = serde_json::from_str(ready.as_ref())
        .map_err(|_| "relay sent an invalid host authentication response")?;
    if ready.kind != "ready" || ready.protocol != Some(RELAY_PROTOCOL) {
        return Err("relay rejected host authentication".into());
    }
    state.set(RelayState::Connected);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| error.to_string())?;
    let local_base = format!("http://{local_address}");
    let mut client_devices: HashMap<String, String> = HashMap::new();
    let mut sessions: HashMap<String, Session> = HashMap::new();
    let mut auth_sync = tokio::time::interval(AUTH_SYNC_INTERVAL);
    auth_sync.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    eprintln!("Choro relay connected for room {}", identity.room_id);

    loop {
        tokio::select! {
            message = stream.next() => {
                let Some(message) = message else { return Ok(()); };
                let message = message.map_err(|error| error.to_string())?;
                let Message::Text(text) = message else { continue; };
                let incoming: RelayIncoming = match serde_json::from_str(text.as_ref()) {
                    Ok(incoming) => incoming,
                    Err(_) => continue,
                };
                if incoming.kind == "client_left" {
                    if let Some(client_id) = incoming.client_id {
                        client_devices.remove(&client_id);
                        sessions.remove(&client_id);
                        publish_presence(presence, &client_devices);
                    }
                    continue;
                }
                if incoming.kind != "data" { continue; }
                let (Some(client_id), Some(payload)) = (incoming.client_id, incoming.payload) else { continue; };
                let envelope: SealedEnvelope = match serde_json::from_str::<SealedEnvelope>(&payload) {
                    Ok(envelope) if envelope.version == PROTOCOL => envelope,
                    _ => continue,
                };
                let response = if envelope.kind == "pair" {
                    handle_pair(&auth, envelope)
                } else if envelope.kind == "session_hello" {
                    handle_session_hello(
                        &auth,
                        &client_id,
                        &mut sessions,
                        envelope,
                    )
                } else if envelope.kind == "request" {
                    handle_request(
                        &client,
                        &local_base,
                        &auth,
                        &client_id,
                        &mut client_devices,
                        &mut sessions,
                        envelope,
                    ).await
                } else {
                    None
                };
                publish_presence(presence, &client_devices);
                let Some(response) = response else { continue; };
                let outgoing = RelayOutgoing { kind: "data", client_id: &client_id, payload: serde_json::to_string(&response).map_err(|error| error.to_string())? };
                sink.send(Message::Text(serde_json::to_string(&outgoing).map_err(|error| error.to_string())?.into()))
                    .await
                    .map_err(|error| error.to_string())?;
            }
            event = events.recv() => {
                let Ok(event) = event else { continue; };
                let event_json = serde_json::to_vec(&json!({ "event": event })).map_err(|error| error.to_string())?;
                let destinations: Vec<String> = sessions.keys().cloned().collect();
                for client_id in destinations {
                    let Some(session) = sessions.get_mut(&client_id) else { continue; };
                    if session.expires_at <= unix_now() || !auth.is_device_active(&session.device_id) { continue; }
                    session.send_sequence += 1;
                    let response = seal_session("event", session, &event_json)?;
                    let outgoing = RelayOutgoing { kind: "data", client_id: &client_id, payload: serde_json::to_string(&response).map_err(|error| error.to_string())? };
                    sink.send(Message::Text(serde_json::to_string(&outgoing).map_err(|error| error.to_string())?.into()))
                        .await
                        .map_err(|error| error.to_string())?;
                }
            }
            _ = auth_sync.tick() => {
                let status = auth.snapshot();
                let sync = RelayAuthSync {
                    kind: "auth_sync",
                    admission_hashes: auth.admission_token_hashes(),
                    pairing_open: status.active_code.is_some(),
                };
                sink.send(Message::Text(serde_json::to_string(&sync).map_err(|error| error.to_string())?.into()))
                    .await
                    .map_err(|error| error.to_string())?;
            }
        }
    }
}

fn publish_presence(presence: &Sender<usize>, client_devices: &HashMap<String, String>) {
    let connected_devices = connected_device_count(client_devices);
    let _ = presence.try_send(connected_devices);
}

fn connected_device_count(client_devices: &HashMap<String, String>) -> usize {
    client_devices.values().collect::<HashSet<_>>().len()
}

fn handle_pair(auth: &RemoteAuth, envelope: SealedEnvelope) -> Option<SealedEnvelope> {
    let active_code = auth.snapshot().active_code?;
    let key = pairing_key(&active_code);
    let plaintext = open(&envelope, &key).ok()?;
    let request: PairRequest = serde_json::from_slice(&plaintext).ok()?;
    let mut transport_key = [0_u8; 32];
    OsRng.fill_bytes(&mut transport_key);
    let encoded_transport_key = URL_SAFE_NO_PAD.encode(transport_key);
    let (status, body) = match auth.pair_with_transport_key(
        &request.code,
        &request.device_name,
        Some(encoded_transport_key.clone()),
    ) {
        Ok(result) => {
            let body = json!({
                "token": result.token,
                "admission_token": result.admission_token,
                "device": result.device,
                "transport_key": encoded_transport_key,
            });
            (200, body)
        }
        Err(error) => (
            pairing_error_status(&error),
            json!({ "error": pairing_error_message(&error) }),
        ),
    };
    let response = TunnelResponse {
        request_id: request.request_id,
        status,
        content_type: "application/json".into(),
        body_base64: BASE64.encode(serde_json::to_vec(&body).ok()?),
    };
    seal(
        "pair_response",
        None,
        None,
        None,
        &key,
        &serde_json::to_vec(&response).ok()?,
    )
    .ok()
}

fn handle_session_hello(
    auth: &RemoteAuth,
    client_id: &str,
    sessions: &mut HashMap<String, Session>,
    envelope: SealedEnvelope,
) -> Option<SealedEnvelope> {
    let device_id = envelope.device_id.clone()?;
    let transport_key = decode_key(&auth.transport_key(&device_id)?)?;
    let plaintext = open(&envelope, &transport_key).ok()?;
    let hello: SessionHello = serde_json::from_slice(&plaintext).ok()?;
    if hello.device_id != device_id
        || hello.session_id.len() < 16
        || hello.session_id.len() > 128
        || auth
            .authorize_device(&hello.token, Some(&device_id))
            .is_none()
    {
        return None;
    }
    let client_public_bytes: [u8; 32] = URL_SAFE_NO_PAD
        .decode(&hello.client_public_key)
        .ok()?
        .try_into()
        .ok()?;
    let client_nonce = URL_SAFE_NO_PAD.decode(&hello.client_nonce).ok()?;
    if client_nonce.len() != 32 {
        return None;
    }

    let server_secret = StaticSecret::random_from_rng(OsRng);
    let server_public = PublicKey::from(&server_secret);
    let mut server_nonce = [0_u8; 32];
    OsRng.fill_bytes(&mut server_nonce);
    let (receive_key, send_key) = derive_session_keys(
        &transport_key,
        &server_secret,
        &client_public_bytes,
        &hello.session_id,
        &client_nonce,
        &server_nonce,
    )
    .ok()?;
    let expires_at = unix_now() + SESSION_TTL_SECS;
    let ready = SessionReady {
        session_id: hello.session_id.clone(),
        server_public_key: URL_SAFE_NO_PAD.encode(server_public.as_bytes()),
        server_nonce: URL_SAFE_NO_PAD.encode(server_nonce),
        expires_at,
    };
    let response = seal(
        "session_ready",
        Some(&device_id),
        Some(&hello.session_id),
        None,
        &transport_key,
        &serde_json::to_vec(&ready).ok()?,
    )
    .ok()?;
    sessions.insert(
        client_id.to_string(),
        Session {
            session_id: hello.session_id,
            device_id,
            token: hello.token,
            receive_key,
            send_key,
            receive_sequence: 0,
            send_sequence: 0,
            expires_at,
        },
    );
    Some(response)
}

fn derive_session_keys(
    transport_key: &[u8; 32],
    server_secret: &StaticSecret,
    client_public_bytes: &[u8; 32],
    session_id: &str,
    client_nonce: &[u8],
    server_nonce: &[u8; 32],
) -> Result<([u8; 32], [u8; 32]), String> {
    let client_public = PublicKey::from(*client_public_bytes);
    let shared = server_secret.diffie_hellman(&client_public);
    if shared.as_bytes().iter().all(|byte| *byte == 0) {
        return Err("invalid client public key".into());
    }
    let mut info = Vec::with_capacity(256);
    info.extend_from_slice(b"choro-relay-session-v2\0");
    info.extend_from_slice(session_id.as_bytes());
    info.push(0);
    info.extend_from_slice(client_public_bytes);
    info.extend_from_slice(PublicKey::from(server_secret).as_bytes());
    info.extend_from_slice(client_nonce);
    info.extend_from_slice(server_nonce);
    let mut output = [0_u8; 64];
    Hkdf::<Sha256>::new(Some(transport_key), shared.as_bytes())
        .expand(&info, &mut output)
        .map_err(|_| "session key derivation failed")?;
    let mut receive_key = [0_u8; 32];
    let mut send_key = [0_u8; 32];
    receive_key.copy_from_slice(&output[..32]);
    send_key.copy_from_slice(&output[32..]);
    Ok((receive_key, send_key))
}

async fn handle_request(
    client: &reqwest::Client,
    local_base: &str,
    auth: &RemoteAuth,
    client_id: &str,
    client_devices: &mut HashMap<String, String>,
    sessions: &mut HashMap<String, Session>,
    envelope: SealedEnvelope,
) -> Option<SealedEnvelope> {
    let session = sessions.get_mut(client_id)?;
    if session.expires_at <= unix_now() || !auth.is_device_active(&session.device_id) {
        sessions.remove(client_id);
        client_devices.remove(client_id);
        return None;
    }
    if !session_accepts_envelope(session, &envelope) {
        return None;
    }
    let plaintext = open(&envelope, &session.receive_key).ok()?;
    session.receive_sequence += 1;
    let request: TunnelRequest = serde_json::from_slice(&plaintext).ok()?;
    let device_id = session.device_id.clone();
    let token = session.token.clone();
    client_devices.insert(client_id.to_string(), device_id.clone());

    let response = if !valid_request(&request) {
        json_response(
            &request.request_id,
            400,
            json!({ "error": "Invalid remote request" }),
        )
    } else if let Some(error) = authorization_error(auth.device_permission(&device_id)?, &request) {
        json_response(&request.request_id, 403, json!({ "error": error }))
    } else {
        forward_request(client, local_base, &request, &token).await
    };
    let session = sessions.get_mut(client_id)?;
    session.send_sequence += 1;
    seal_session("response", session, &serde_json::to_vec(&response).ok()?).ok()
}

fn session_accepts_envelope(session: &Session, envelope: &SealedEnvelope) -> bool {
    envelope.device_id.as_deref() == Some(session.device_id.as_str())
        && envelope.session_id.as_deref() == Some(session.session_id.as_str())
        && envelope.sequence == Some(session.receive_sequence + 1)
}

async fn forward_request(
    client: &reqwest::Client,
    local_base: &str,
    request: &TunnelRequest,
    token: &str,
) -> TunnelResponse {
    let method = if request.method == "GET" {
        reqwest::Method::GET
    } else {
        reqwest::Method::POST
    };
    let mut builder = client
        .request(method, format!("{local_base}{}", request.path))
        .bearer_auth(token)
        .header(reqwest::header::CONTENT_TYPE, "application/json");
    if let Some(body) = &request.body {
        builder = builder.body(body.clone());
    }
    match builder.send().await {
        Ok(response) => {
            let status = response.status().as_u16();
            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or("application/octet-stream")
                .to_string();
            match response.bytes().await {
                Ok(bytes) => TunnelResponse {
                    request_id: request.request_id.clone(),
                    status,
                    content_type,
                    body_base64: BASE64.encode(bytes),
                },
                Err(error) => json_response(
                    &request.request_id,
                    502,
                    json!({ "error": error.to_string() }),
                ),
            }
        }
        Err(_) => json_response(
            &request.request_id,
            503,
            json!({ "error": "Choro Desktop API is unavailable" }),
        ),
    }
}

fn valid_request(request: &TunnelRequest) -> bool {
    matches!(request.method.as_str(), "GET" | "POST")
        && allowlisted_route(&request.method, &request.path)
        && !request.path.contains("..")
        && request.path.len() <= MAX_PATH_BYTES
        && request.body.as_ref().map_or(0, String::len) <= MAX_BODY_BYTES
        && request.request_id.len() <= 128
}

fn allowlisted_route(method: &str, path: &str) -> bool {
    let route = path.split('?').next().unwrap_or_default();
    let segments = route.trim_matches('/').split('/').collect::<Vec<_>>();
    match method {
        "GET" => matches!(
            segments.as_slice(),
            ["v1", "health"]
                | ["v1", "configuration"]
                | ["v1", "projects"]
                | ["v1", "projects", _, "agents"]
                | ["v1", "agents", _]
                | ["v1", "agents", _, "visualizations", "snapshot"]
        ),
        "POST" => matches!(
            segments.as_slice(),
            ["v1", "agents"]
                | ["v1", "agents", _, "messages"]
                | ["v1", "agents", _, "configuration"]
                | ["v1", "agents", _, "stop"]
                | ["v1", "agents", _, "questions", _, "answer"]
                | ["v1", "agents", _, "plan", "resolve"]
                | ["v1", "agents", _, "plan", "dismiss"]
                | ["v1", "agents", _, "approvals", _, "resolve"]
        ),
        _ => false,
    }
}

fn authorization_error(
    permission: DevicePermission,
    request: &TunnelRequest,
) -> Option<&'static str> {
    if request.method == "GET" {
        return None;
    }
    if permission == DevicePermission::ViewOnly {
        return Some(
            "This iPhone is set to View only. Change it to Control in Desktop Settings → Remote access.",
        );
    }
    if permission == DevicePermission::FullAccess {
        return None;
    }
    if request.path.contains("/approvals/") {
        return Some(
            "This action requires Full access for this iPhone. Change its permission in Desktop Settings → Remote access.",
        );
    }
    let requests_full_access = request.body.as_deref().is_some_and(|body| {
        serde_json::from_str::<serde_json::Value>(body)
            .ok()
            .and_then(|value| {
                value
                    .get("access_mode")
                    .and_then(|mode| mode.as_str())
                    .map(str::to_owned)
            })
            .is_some_and(|mode| mode.trim() == "full_access")
    });
    requests_full_access.then_some(
        "Full access was selected for the agent, but this iPhone has Control permission. Choose Approve for me, or grant the phone Full access in Desktop Settings → Remote access.",
    )
}

fn json_response(request_id: &str, status: u16, value: serde_json::Value) -> TunnelResponse {
    TunnelResponse {
        request_id: request_id.to_string(),
        status,
        content_type: "application/json".into(),
        body_base64: BASE64.encode(serde_json::to_vec(&value).unwrap_or_default()),
    }
}

fn pairing_key(code: &str) -> [u8; 32] {
    let normalized: String = code
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_uppercase)
        .collect();
    let mut digest = Sha256::new();
    digest.update(b"choro-pair-v1:");
    digest.update(normalized.as_bytes());
    digest.finalize().into()
}

fn aad(
    kind: &str,
    device_id: Option<&str>,
    session_id: Option<&str>,
    sequence: Option<u64>,
) -> Vec<u8> {
    format!(
        "choro-relay-v2|{kind}|{}|{}|{}",
        device_id.unwrap_or("pair"),
        session_id.unwrap_or("none"),
        sequence.map_or_else(|| "none".into(), |value| value.to_string())
    )
    .into_bytes()
}

fn open(envelope: &SealedEnvelope, key: &[u8; 32]) -> Result<Vec<u8>, String> {
    let nonce = URL_SAFE_NO_PAD
        .decode(&envelope.nonce)
        .map_err(|_| "invalid nonce")?;
    if nonce.len() != 24 {
        return Err("invalid nonce".into());
    }
    let ciphertext = BASE64
        .decode(&envelope.ciphertext)
        .map_err(|_| "invalid ciphertext")?;
    XChaCha20Poly1305::new(Key::from_slice(key))
        .decrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: &ciphertext,
                aad: &aad(
                    &envelope.kind,
                    envelope.device_id.as_deref(),
                    envelope.session_id.as_deref(),
                    envelope.sequence,
                ),
            },
        )
        .map_err(|_| "authentication failed".into())
}

fn seal(
    kind: &str,
    device_id: Option<&str>,
    session_id: Option<&str>,
    sequence: Option<u64>,
    key: &[u8; 32],
    plaintext: &[u8],
) -> Result<SealedEnvelope, String> {
    let mut nonce = [0_u8; 24];
    OsRng.fill_bytes(&mut nonce);
    let ciphertext = XChaCha20Poly1305::new(Key::from_slice(key))
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad: &aad(kind, device_id, session_id, sequence),
            },
        )
        .map_err(|_| "encryption failed")?;
    Ok(SealedEnvelope {
        version: PROTOCOL,
        kind: kind.to_string(),
        device_id: device_id.map(str::to_string),
        session_id: session_id.map(str::to_string),
        sequence,
        nonce: URL_SAFE_NO_PAD.encode(nonce),
        ciphertext: BASE64.encode(ciphertext),
    })
}

fn seal_session(kind: &str, session: &Session, plaintext: &[u8]) -> Result<SealedEnvelope, String> {
    seal(
        kind,
        Some(&session.device_id),
        Some(&session.session_id),
        Some(session.send_sequence),
        &session.send_key,
        plaintext,
    )
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn decode_key(value: &str) -> Option<[u8; 32]> {
    let bytes = URL_SAFE_NO_PAD.decode(value).ok()?;
    bytes.try_into().ok()
}

#[cfg(target_os = "macos")]
fn relay_keychain_get() -> Option<String> {
    let bytes = security_framework::passwords::get_generic_password(
        &relay_keychain_service(),
        RELAY_KEYCHAIN_ACCOUNT,
    )
    .ok()?;
    String::from_utf8(bytes).ok()
}

#[cfg(not(target_os = "macos"))]
fn relay_keychain_get() -> Option<String> {
    None
}

#[cfg(target_os = "macos")]
fn relay_keychain_set(secret: &str) -> Result<(), String> {
    security_framework::passwords::set_generic_password(
        &relay_keychain_service(),
        RELAY_KEYCHAIN_ACCOUNT,
        secret.as_bytes(),
    )
    .map_err(|error| error.to_string())
}

#[cfg(not(target_os = "macos"))]
fn relay_keychain_set(_secret: &str) -> Result<(), String> {
    Err("Keychain is unavailable".into())
}

fn pairing_error_status(error: &PairingError) -> u16 {
    match error {
        PairingError::InvalidCode => 401,
        PairingError::TooManyAttempts => 429,
        PairingError::DeviceLimit => 409,
        PairingError::NotActive | PairingError::Expired => 409,
        PairingError::Storage(_) => 500,
    }
}

fn pairing_error_message(error: &PairingError) -> String {
    match error {
        PairingError::NotActive => "Start pairing from Choro Desktop first".into(),
        PairingError::Expired => "Pairing code expired; start a new pairing session".into(),
        PairingError::InvalidCode => "Pairing code is incorrect".into(),
        PairingError::TooManyAttempts => "Too many attempts; start pairing again on the Mac".into(),
        PairingError::DeviceLimit => "Revoke an old phone before pairing another one".into(),
        PairingError::Storage(error) => format!("Could not save paired device: {error}"),
    }
}

fn write_private_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension(format!("tmp-{}", Uuid::new_v4().simple()));
    let mut options = fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&temporary, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signature, Verifier as _, VerifyingKey};

    #[test]
    fn legacy_relay_seed_becomes_a_non_exported_signing_identity() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("relay-identity.json");
        let seed = [19_u8; 32];
        fs::write(
            &path,
            serde_json::to_vec(&json!({
                "host_secret": URL_SAFE_NO_PAD.encode(seed),
            }))
            .unwrap(),
        )
        .unwrap();

        let identity = RelayIdentity::load_or_create(&path, false);
        assert_eq!(identity.signing_key, seed);
        assert_eq!(
            identity.room_id,
            URL_SAFE_NO_PAD.encode(SigningKey::from_bytes(&seed).verifying_key())
        );

        let nonce = [23_u8; 32];
        assert_eq!(
            identity.room_id,
            "Zs1gi5KLiOUODv6qM_rxxDzv4HKUsLh-n-CrpqPPdjM"
        );
        assert_eq!(
            identity.sign_challenge(&nonce),
            "VW5Q6EbdfY5MJmLqDc7yHye9_X_uI7mQb2wvbZBoSIA_Uq0U6iaVjYwqNBmIfWPR66iCXdTW0WpsTAGYJSBaCg"
        );
        let signature_bytes: [u8; 64] = URL_SAFE_NO_PAD
            .decode(identity.sign_challenge(&nonce))
            .unwrap()
            .try_into()
            .unwrap();
        let verifying_bytes: [u8; 32] = URL_SAFE_NO_PAD
            .decode(&identity.room_id)
            .unwrap()
            .try_into()
            .unwrap();
        VerifyingKey::from_bytes(&verifying_bytes)
            .unwrap()
            .verify(
                &host_auth_message(&identity.room_id, &nonce),
                &Signature::from_bytes(&signature_bytes),
            )
            .unwrap();

        let persisted = fs::read_to_string(path).unwrap();
        assert!(persisted.contains("signing_key"));
        assert!(!persisted.contains("host_secret"));
    }

    #[test]
    fn envelope_round_trip_authenticates_metadata() {
        let key = [7_u8; 32];
        let sealed = seal(
            "request",
            Some("device"),
            Some("session"),
            Some(1),
            &key,
            b"secret",
        )
        .unwrap();
        assert_eq!(open(&sealed, &key).unwrap(), b"secret");
        let mut tampered = sealed;
        tampered.device_id = Some("other".into());
        assert!(open(&tampered, &key).is_err());
    }

    #[test]
    fn decrypts_the_mobile_javascript_test_vector() {
        let envelope = SealedEnvelope {
            version: PROTOCOL,
            kind: "request".into(),
            device_id: Some("device".into()),
            session_id: None,
            sequence: None,
            nonce: "CQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJ".into(),
            ciphertext: "zReB+BnejoydAqYv70po8ux2eSC7Hw==".into(),
        };
        assert_eq!(open(&envelope, &[7_u8; 32]).unwrap(), b"secret");
    }

    #[test]
    fn session_keys_match_the_mobile_javascript_vector() {
        let server_secret = StaticSecret::from([3_u8; 32]);
        let client_public = PublicKey::from(&StaticSecret::from([2_u8; 32]));
        let (receive, send) = derive_session_keys(
            &[7_u8; 32],
            &server_secret,
            client_public.as_bytes(),
            "session-test",
            &[4_u8; 32],
            &[5_u8; 32],
        )
        .unwrap();
        assert_eq!(
            format!("{}{}", hex(&receive), hex(&send)),
            "f940b758b25833e6a19acd95f228a771f985e341974b22b51f8c098031d3bc69ac9b2488d0580c1432573387e94437d1cc396ab3e2310ef693ca284224a5333c"
        );
    }

    #[test]
    fn session_rejects_replayed_and_out_of_order_sequences() {
        let mut session = Session {
            session_id: "session-test".into(),
            device_id: "device-test".into(),
            token: "token-test".into(),
            receive_key: [1_u8; 32],
            send_key: [2_u8; 32],
            receive_sequence: 0,
            send_sequence: 0,
            expires_at: unix_now() + 60,
        };
        let first = seal(
            "request",
            Some(&session.device_id),
            Some(&session.session_id),
            Some(1),
            &session.receive_key,
            b"{}",
        )
        .unwrap();
        let third = seal(
            "request",
            Some(&session.device_id),
            Some(&session.session_id),
            Some(3),
            &session.receive_key,
            b"{}",
        )
        .unwrap();

        assert!(session_accepts_envelope(&session, &first));
        session.receive_sequence += 1;
        assert!(!session_accepts_envelope(&session, &first));
        assert!(!session_accepts_envelope(&session, &third));
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn control_permission_rejects_full_access_and_approval_resolution() {
        let full_access = TunnelRequest {
            request_id: "1".into(),
            method: "POST".into(),
            path: "/v1/agents".into(),
            body: Some(r#"{"access_mode":"full_access"}"#.into()),
        };
        assert!(authorization_error(DevicePermission::Control, &full_access).is_some());
        let approval = TunnelRequest {
            request_id: "2".into(),
            method: "POST".into(),
            path: "/v1/agents/a/approvals/b/resolve".into(),
            body: Some("{}".into()),
        };
        assert!(authorization_error(DevicePermission::Control, &approval).is_some());
        let message = TunnelRequest {
            request_id: "3".into(),
            method: "POST".into(),
            path: "/v1/agents/a/messages".into(),
            body: Some(r#"{"text":"hello"}"#.into()),
        };
        assert!(authorization_error(DevicePermission::Control, &message).is_none());
    }

    #[test]
    fn relay_rejects_whitespace_full_access_for_control_devices() {
        for path in ["/v1/agents", "/v1/agents/a/configuration"] {
            for mode in [
                "full_access",
                " full_access ",
                "\tfull_access\r\n",
                "\u{2003}full_access\u{a0}",
            ] {
                let request = TunnelRequest {
                    request_id: "permission-regression".into(),
                    method: "POST".into(),
                    path: path.into(),
                    body: Some(json!({ "access_mode": mode }).to_string()),
                };
                assert!(valid_request(&request));
                assert!(authorization_error(DevicePermission::Control, &request).is_some());
                assert!(authorization_error(DevicePermission::ViewOnly, &request).is_some());
                assert!(authorization_error(DevicePermission::FullAccess, &request).is_none());
            }
        }
    }

    #[test]
    fn tunnel_exposes_only_the_explicit_mobile_api() {
        assert!(allowlisted_route("GET", "/v1/projects"));
        assert!(allowlisted_route("POST", "/v1/agents/a/questions/q/answer"));
        assert!(!allowlisted_route("GET", "/v1/events"));
        assert!(!allowlisted_route("POST", "/v1/pairing/complete"));
        assert!(!allowlisted_route("GET", "/v1/future-admin-endpoint"));
    }

    #[test]
    fn presence_counts_each_connected_device_once() {
        let clients = HashMap::from([
            ("client-a".into(), "phone-1".into()),
            ("client-b".into(), "phone-1".into()),
            ("client-c".into(), "phone-2".into()),
        ]);
        assert_eq!(connected_device_count(&clients), 2);
    }
}
