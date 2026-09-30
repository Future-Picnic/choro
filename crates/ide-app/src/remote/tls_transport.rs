//! End-to-end TLS over the opaque v2 relay. The relay never receives a TLS key.
use std::{collections::HashMap, net::SocketAddr, sync::Arc, time::Duration};

use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine as _,
};
use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::{broadcast, mpsc},
    task::JoinSet,
};
use tokio_rustls::TlsAcceptor;
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{
        client::IntoClientRequest, http::header::SEC_WEBSOCKET_PROTOCOL, protocol::WebSocketConfig,
        Message,
    },
};
use uuid::Uuid;

use super::{
    dto::RemoteEvent,
    forwarding::*,
    relay::RelayIdentity,
    server::{RelayPresence, RelayStateStore},
    tls_identity::TlsIdentity,
    RelayState, RemoteAuth,
};

const CHUNK: usize = 64 * 1024;
const MAX_FRAME: usize = 16 * 1024 * 1024;
const IO_DEADLINE: Duration = Duration::from_secs(20);
const QUEUE_DEADLINE: Duration = Duration::from_secs(5);
const SESSION_LIFETIME: Duration = Duration::from_secs(30 * 60);

#[derive(Clone, Default)]
pub(super) struct TlsStatus(Arc<Mutex<(Option<String>, Option<String>)>>);

impl TlsStatus {
    pub(super) fn error(&self) -> Option<String> {
        self.0.lock().1.clone()
    }

    pub(super) fn pairing_payload(
        &self,
        room_id: &str,
        auth: &RemoteAuth,
    ) -> Result<String, String> {
        let status = self.0.lock();
        let authority = status.0.as_ref().ok_or_else(|| {
            status
                .1
                .clone()
                .unwrap_or_else(|| "Connect to prepare secure pairing".into())
        })?;
        let snapshot = auth.snapshot();
        let code = snapshot.active_code.ok_or("Start a new pairing session")?;
        let expires = snapshot.expires_at.ok_or("Pairing session has expired")?;
        let origin = super::relay::configured_url().ok_or("Relay is not configured")?;
        if origin != "https://relay.choro.dev" {
            return Err("The iPhone release requires the production Choro relay".into());
        }
        let value = json!({"version":3,"relayUrl":origin,"roomId":room_id,"authority":authority,"code":code,"expiresAt":expires});
        Ok(format!(
            "CHORO3:{}",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&value).map_err(|e| e.to_string())?)
        ))
    }
}

pub(super) async fn run(
    relay_url: String,
    identity: RelayIdentity,
    local: SocketAddr,
    auth: RemoteAuth,
    events: broadcast::Sender<RemoteEvent>,
    presence: RelayPresence,
    state: RelayStateStore,
    status: TlsStatus,
) {
    let mut backoff = Duration::from_secs(1);
    let mut tls_identity = None;
    loop {
        state.set(RelayState::Connecting);
        if tls_identity.is_none() {
            match TlsIdentity::load_default() {
                Ok(identity) => {
                    *status.0.lock() = (Some(STANDARD.encode(identity.authority_der())), None);
                    tls_identity = Some(identity);
                }
                Err(error) => {
                    *status.0.lock() = (None, Some(format!("Secure identity unavailable: {error}. Unlock the Mac Keychain and reconnect.")));
                    state.set(RelayState::Disconnected);
                    // Recover when Keychain becomes available. Existing-identity
                    // failures must never generate a replacement.
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    continue;
                }
            }
        }
        let config = tls_identity.as_mut().expect("loaded above").server_config();
        status.0.lock().1 = None;
        let result = match config {
            Ok(config) => {
                run_connection(
                    &relay_url,
                    &identity,
                    local,
                    auth.clone(),
                    events.clone(),
                    presence.clone(),
                    &state,
                    TlsAcceptor::from(config),
                )
                .await
            }
            Err(error) => Err(format!("Could not renew secure identity: {error}")),
        };
        if let Err(error) = result {
            eprintln!("Choro TLS relay unavailable: {error}");
            status.0.lock().1 = Some(error);
        }
        state.set(RelayState::Reconnecting);
        presence.set(0);
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(20));
    }
}

struct StreamSlot {
    input: mpsc::Sender<Vec<u8>>,
    abort: tokio::task::AbortHandle,
}

async fn run_connection(
    relay_url: &str,
    identity: &RelayIdentity,
    local: SocketAddr,
    auth: RemoteAuth,
    events: broadcast::Sender<RemoteEvent>,
    presence: RelayPresence,
    state: &RelayStateStore,
    acceptor: TlsAcceptor,
) -> Result<(), String> {
    let base = loopback_base(local)?;
    let client = forwarding_client()?;
    let ws_base = relay_url
        .replacen("https://", "wss://", 1)
        .replacen("http://", "ws://", 1);
    let mut request = format!("{ws_base}/v2/relay?role=host&room={}", identity.room_id())
        .into_client_request()
        .map_err(|e| e.to_string())?;
    request
        .headers_mut()
        .insert(SEC_WEBSOCKET_PROTOCOL, "choro-relay-v2".parse().unwrap());
    let mut ws_config = WebSocketConfig::default();
    ws_config.max_message_size = Some(CHUNK + 16);
    ws_config.max_frame_size = Some(CHUNK + 16);
    ws_config.write_buffer_size = CHUNK;
    ws_config.max_write_buffer_size = 4 * CHUNK;
    let (mut socket, response) = tokio::time::timeout(
        IO_DEADLINE,
        connect_async_with_config(request, Some(ws_config), false),
    )
    .await
    .map_err(|_| "Relay connection timed out")?
    .map_err(|e| e.to_string())?;
    if response
        .headers()
        .get(SEC_WEBSOCKET_PROTOCOL)
        .and_then(|v| v.to_str().ok())
        != Some("choro-relay-v2")
    {
        return Err("Relay did not negotiate v2".into());
    }
    let challenge = control(socket.next()).await?;
    if challenge["type"] != "challenge" || challenge["protocol"] != 4 {
        return Err("Relay TLS protocol is incompatible".into());
    }
    let nonce: [u8; 32] = URL_SAFE_NO_PAD
        .decode(challenge["nonce"].as_str().ok_or("Missing challenge")?)
        .map_err(|_| "Invalid nonce")?
        .try_into()
        .map_err(|_| "Invalid nonce length")?;
    send_relay(&mut socket, Message::text(json!({"type":"host_auth","nonce":URL_SAFE_NO_PAD.encode(nonce),"signature":identity.sign_tls_challenge(&nonce)}).to_string())).await?;
    let ready = control(socket.next()).await?;
    if ready["type"] != "ready" || ready["protocol"] != 4 {
        return Err("Relay rejected TLS host".into());
    }
    let generation = Uuid::parse_str(
        ready["generation"]
            .as_str()
            .ok_or("Missing relay generation")?,
    )
    .map_err(|_| "Invalid relay generation")?;
    state.set(RelayState::Connected);
    let (mut sink, mut source) = socket.split();
    let (output, mut output_rx) = mpsc::channel::<Message>(16);
    let (done_tx, mut done_rx) = mpsc::channel::<Uuid>(4);
    let (device_tx, mut device_rx) = mpsc::channel::<(Uuid, String)>(4);
    let mut streams: HashMap<Uuid, StreamSlot> = HashMap::new();
    let mut devices: HashMap<Uuid, String> = HashMap::new();
    // JoinSet aborts all owned stream tasks on disconnect; no detached old-generation work.
    let mut workers = JoinSet::new();
    let mut sync = tokio::time::interval(Duration::from_secs(1));
    sync.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let connection_lifetime = tokio::time::sleep(Duration::from_secs(12 * 60 * 60));
    tokio::pin!(connection_lifetime);
    loop {
        tokio::select! {
            _ = &mut connection_lifetime => return Ok(()), // Refresh leaf/config even on an idle Mac.
            message = source.next() => {
                let message = message.ok_or("Relay disconnected")?.map_err(|e|e.to_string())?;
                match message {
                    Message::Binary(bytes) => {
                        if !(17..=CHUNK+16).contains(&bytes.len()) { return Err("Invalid TLS relay chunk".into()); }
                        let id = Uuid::from_slice(&bytes[..16]).map_err(|_|"Invalid stream identifier")?;
                        if let Some(slot) = streams.get(&id) {
                            if !matches!(tokio::time::timeout(QUEUE_DEADLINE, slot.input.send(bytes[16..].to_vec())).await, Ok(Ok(()))) {
                                if let Some(slot) = streams.remove(&id) { slot.abort.abort(); }
                                devices.remove(&id);
                                send_relay(&mut sink, close_message(id, generation)).await?;
                            }
                        } // A closed stream cannot be resurrected by late ciphertext.
                    }
                    Message::Text(raw) => {
                        let value: Value = serde_json::from_str(&raw).map_err(|_|"Invalid relay control")?;
                        if value["generation"].as_str() != Some(generation.to_string().as_str()) { return Err("Stale relay generation".into()); }
                        let id = Uuid::parse_str(value["client_id"].as_str().ok_or("Missing stream ID")?).map_err(|_|"Invalid stream ID")?;
                        match value["type"].as_str() {
                            Some("stream_open") => {
                                if streams.contains_key(&id) || streams.len() >= 4 { return Err("Duplicate or excessive TLS stream".into()); }
                                let purpose = value["purpose"].as_str().ok_or("Missing stream purpose")?;
                                if !matches!(purpose,"pair"|"device") { return Err("Invalid stream purpose".into()); }
                                let pairing = purpose == "pair";
                                let (input, input_rx) = mpsc::channel(4);
                                let out = output.clone(); let done = done_tx.clone(); let admitted = device_tx.clone();
                                let acceptor = acceptor.clone(); let auth = auth.clone(); let events = events.subscribe();
                                let client = client.clone(); let base = base.clone();
                                let abort = workers.spawn(async move {
                                    let _ = run_stream(id, input_rx, out, acceptor, auth, events, client, base, pairing, admitted).await;
                                    let _ = done.send(id).await;
                                });
                                streams.insert(id,StreamSlot{input,abort});
                            }
                            Some("stream_close") => { if let Some(slot)=streams.remove(&id){slot.abort.abort();} devices.remove(&id); }
                            _ => return Err("Unexpected relay control".into()),
                        }
                    }
                    Message::Ping(data) => send_relay(&mut sink, Message::Pong(data)).await?,
                    Message::Pong(_) => {},
                    Message::Close(_) => return Ok(()),
                    _ => return Err("Unexpected relay frame".into()),
                }
            }
            Some(message) = output_rx.recv() => {
                // Output queued by a retired stream is discarded before reaching the relay.
                if let Message::Binary(bytes) = &message {
                    if !Uuid::from_slice(&bytes[..16]).ok().is_some_and(|id|streams.contains_key(&id)) { continue; }
                }
                tokio::time::timeout(QUEUE_DEADLINE,sink.send(message)).await.map_err(|_|"Relay send stalled")?.map_err(|e|e.to_string())?;
            }
            Some(id) = done_rx.recv() => {
                if let Some(slot)=streams.remove(&id){slot.abort.abort();}
                devices.remove(&id);
                send_relay(&mut sink, close_message(id,generation)).await?;
            }
            Some((id,device)) = device_rx.recv() => { if streams.contains_key(&id) { devices.insert(id,device); } }
            _ = workers.join_next(), if !workers.is_empty() => {},
            _ = sync.tick() => {
                let revoked: Vec<_> = devices.iter().filter(|(_,device)|!auth.is_device_active(device)).map(|(id,_)|*id).collect();
                for id in revoked { if let Some(slot)=streams.remove(&id){slot.abort.abort();} devices.remove(&id); send_relay(&mut sink, close_message(id,generation)).await?; }
                let message = json!({"type":"auth_sync","admission_hashes":auth.admission_token_hashes(),"pending_admission_hashes":auth.pending_admission_token_hashes(),"pairing_open":auth.snapshot().active_code.is_some()});
                tokio::time::timeout(QUEUE_DEADLINE,sink.send(Message::text(message.to_string()))).await.map_err(|_|"Relay auth sync stalled")?.map_err(|e|e.to_string())?;
            }
        }
        presence.set(
            devices
                .values()
                .collect::<std::collections::HashSet<_>>()
                .len(),
        );
    }
}

async fn send_relay<S>(sink: &mut S, message: Message) -> Result<(), String>
where
    S: futures_util::Sink<Message> + Unpin,
    S::Error: std::fmt::Display,
{
    tokio::time::timeout(QUEUE_DEADLINE, sink.send(message))
        .await
        .map_err(|_| "Relay send stalled".to_string())?
        .map_err(|e| e.to_string())
}

fn close_message(id: Uuid, generation: Uuid) -> Message {
    Message::text(json!({"type":"stream_close","client_id":id,"generation":generation}).to_string())
}

async fn control<F>(future: F) -> Result<Value, String>
where
    F: std::future::Future<Output = Option<Result<Message, tokio_tungstenite::tungstenite::Error>>>,
{
    let value = tokio::time::timeout(Duration::from_secs(5), future)
        .await
        .map_err(|_| "Relay handshake timed out")?
        .ok_or("Relay closed")?
        .map_err(|e| e.to_string())?;
    if let Message::Text(text) = value {
        serde_json::from_str(&text).map_err(|_| "Invalid relay control".into())
    } else {
        Err("Invalid relay handshake frame".into())
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_stream(
    id: Uuid,
    mut input: mpsc::Receiver<Vec<u8>>,
    output: mpsc::Sender<Message>,
    acceptor: TlsAcceptor,
    auth: RemoteAuth,
    events: broadcast::Receiver<RemoteEvent>,
    client: reqwest::Client,
    base: String,
    pairing: bool,
    admitted: mpsc::Sender<(Uuid, String)>,
) -> Result<(), String> {
    let (network, tls_io) = tokio::io::duplex(CHUNK);
    let (mut reader, mut writer) = tokio::io::split(network);
    let inward = async move {
        while let Some(chunk) = input.recv().await {
            writer.write_all(&chunk).await.map_err(|e| e.to_string())?;
        }
        Err::<(), String>("Relay stream closed".into())
    };
    let outward = async move {
        let mut buffer = vec![0u8; CHUNK];
        loop {
            let size = reader.read(&mut buffer).await.map_err(|e| e.to_string())?;
            if size == 0 {
                return Err::<(), String>("TLS stream closed".into());
            }
            let mut bytes = Vec::with_capacity(16 + size);
            bytes.extend_from_slice(id.as_bytes());
            bytes.extend_from_slice(&buffer[..size]);
            tokio::time::timeout(QUEUE_DEADLINE, output.send(Message::Binary(bytes.into())))
                .await
                .map_err(|_| "TLS output stalled")?
                .map_err(|_| "Relay closed")?;
        }
    };
    let application = async move {
        let tls = tokio::time::timeout(IO_DEADLINE, acceptor.accept(tls_io))
            .await
            .map_err(|_| "TLS handshake timed out")?
            .map_err(|_| "TLS handshake rejected")?;
        if tls.get_ref().1.alpn_protocol() != Some(b"choro-remote/3") {
            return Err("Invalid TLS application protocol".into());
        }
        serve_application(id, tls, auth, events, client, base, pairing, admitted).await
    };
    tokio::select! {result=inward=>result,result=outward=>result,result=application=>result}
}

#[allow(clippy::too_many_arguments)]
async fn serve_application<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    id: Uuid,
    tls: S,
    auth: RemoteAuth,
    mut events: broadcast::Receiver<RemoteEvent>,
    client: reqwest::Client,
    base: String,
    pairing: bool,
    admitted: mpsc::Sender<(Uuid, String)>,
) -> Result<(), String> {
    let (mut reader, mut writer) = tokio::io::split(tls);
    let (incoming_tx, mut incoming) = mpsc::channel(2);
    let (outgoing, mut outgoing_rx) = mpsc::channel::<Value>(2);
    let mut io_tasks = JoinSet::new();
    io_tasks.spawn(async move {
        loop {
            let value = read_frame(&mut reader).await?;
            incoming_tx
                .send(value)
                .await
                .map_err(|_| "Session closed".to_string())?;
        }
    });
    io_tasks.spawn(async move {
        while let Some(value) = outgoing_rx.recv().await {
            write_frame(&mut writer, &value).await?;
        }
        Ok::<(), String>(())
    });
    let deadline = tokio::time::sleep(SESSION_LIFETIME);
    tokio::pin!(deadline);
    let mut authorization: Option<(String, String)> = None;
    let mut checks = tokio::time::interval(Duration::from_secs(1));
    checks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut pending = JoinSet::new();
    let mut seen = std::collections::HashSet::new();
    let mut authenticated = false;
    let unauthenticated_deadline = tokio::time::sleep(Duration::from_secs(30));
    tokio::pin!(unauthenticated_deadline);
    loop {
        tokio::select! {
            _=&mut deadline => return Err("Session expired; reconnect".into()),
            _=&mut unauthenticated_deadline, if !authenticated => return Err("Session authentication timed out".into()),
            _=io_tasks.join_next() => return Err("TLS connection closed".into()),
            _=checks.tick() => { if let Some((device,token))=&authorization {if auth.authorize_device(token,Some(device)).is_none(){return Err("Device access revoked or expired".into());}} }
            event=events.recv(), if authorization.is_some() => {
                match event {Ok(event)=>send_application(&outgoing,json!({"type":"event","event":event})).await?,Err(broadcast::error::RecvError::Lagged(_))=>send_application(&outgoing,json!({"type":"event","event":RemoteEvent::HostSnapshotChanged})).await?,Err(_)=>return Err("Event stream closed".into())}
            }
            Some(result)=pending.join_next(), if !pending.is_empty() => {
                let value=result.map_err(|_|"Remote forwarding interrupted")?;
                // Authorization can change while the local API is responding.
                if let Some((device,token))=&authorization {if auth.authorize_device(token,Some(device)).is_none(){return Err("Device access revoked".into());}}
                send_application(&outgoing,value).await?;
            }
            Some(message)=incoming.recv(), if pending.len()<16 => {
                let request_id=message["request_id"].as_str().filter(|s|!s.is_empty()&&s.len()<=128).ok_or("Invalid request ID")?.to_string();
                if !seen.insert(request_id.clone()) {return Err("Repeated application request ID".into());}
                if seen.len()>10_000{return Err("Session request limit reached; reconnect".into());}
                let reply=match message["type"].as_str() {
                    Some("pair") if pairing && authorization.is_none()=>{
                        let code=message["code"].as_str().filter(|v|v.len()<=128).ok_or("Invalid pairing code")?;
                        let name=message["device_name"].as_str().unwrap_or("iPhone");
                        let previous=message["previous_token"].as_str();
                        match auth.prepare_pairing(code,name,previous) {
                            Ok(prepared)=>json!({"type":"paired","request_id":request_id,"transaction_id":prepared.transaction_id,"token":prepared.token,"admission_token":prepared.admission_token,"device":prepared.device}),
                            Err(error)=>error_response(&request_id,&super::relay::pairing_error_message(&error),"pairing_failed"),
                        }
                    }
                    Some("pair_commit") if authorization.is_none()=>{
                        let transaction=message["transaction_id"].as_str().filter(|v|v.len()<=128).ok_or("Invalid pairing transaction")?;
                        let token=message["token"].as_str().filter(|v|v.len()<=256).ok_or("Invalid pairing token")?;
                        match auth.commit_pairing(transaction,token) {
                            Ok(device)=>{authenticated=true;json!({"type":"pair_committed","request_id":request_id,"device_id":device.id})},
                            Err(error)=>error_response(&request_id,&super::relay::pairing_error_message(&error),"pairing_failed"),
                        }
                    }
                    Some("authenticate") if !pairing && authorization.is_none()=>{
                        let device=message["device_id"].as_str().ok_or("Invalid device")?;let token=message["token"].as_str().ok_or("Invalid token")?;
                        if let Some(device)=auth.authorize_device(token,Some(device)) {
                            authorization=Some((device.id.clone(),token.to_string()));authenticated=true;
                            admitted.send((id,device.id.clone())).await.map_err(|_|"Relay disconnected")?;
                            json!({"type":"authenticated","request_id":request_id,"device_id":device.id,"expires_at":device.expires_at})
                        }else{error_response(&request_id,"Device access revoked or expired","unauthorized")}
                    }
                    Some("request") if authorization.is_some()=>{
                        let request:TunnelRequest=serde_json::from_value(message).map_err(|_|"Invalid application request")?;
                        let (device,token)=authorization.as_ref().unwrap();
                        let permission=auth.authorize_device(token,Some(device)).ok_or("Device access revoked")?.permission;
                        let rejection=if !valid_request(&request){Some((400,"Invalid remote request"))}else{authorization_error(permission,&request).map(|e|(403,e))};
                        if let Some((status,error))=rejection {
                            response_value(json_response(&request_id,status,json!({"error":error})))
                        }else{
                            let client=client.clone();let base=base.clone();let token=token.clone();
                            pending.spawn(async move{response_value(forward_request(&client,&base,&request,&token).await)});
                            continue;
                        }
                    }
                    _=>return Err("Unauthorized or unknown application message".into()),
                };
                send_application(&outgoing,reply).await?;
            }
        }
    }
}

fn response_value(response: TunnelResponse) -> Value {
    let mut value = serde_json::to_value(response).expect("Serializable response");
    value["type"] = json!("response");
    value
}
fn error_response(request: &str, message: &str, code: &str) -> Value {
    json!({"type":"error","request_id":request,"message":message,"code":code})
}
async fn send_application(sender: &mpsc::Sender<Value>, value: Value) -> Result<(), String> {
    tokio::time::timeout(QUEUE_DEADLINE, sender.send(value))
        .await
        .map_err(|_| "Remote consumer stalled")?
        .map_err(|_| "TLS closed".into())
}

async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Value, String> {
    let size = reader.read_u32().await.map_err(|e| e.to_string())? as usize;
    if size == 0 || size > MAX_FRAME {
        return Err("Oversized or empty TLS application frame".into());
    }
    let mut bytes = vec![0u8; size];
    tokio::time::timeout(IO_DEADLINE, reader.read_exact(&mut bytes))
        .await
        .map_err(|_| "Application frame stalled")?
        .map_err(|e| e.to_string())?;
    serde_json::from_slice(&bytes).map_err(|_| "Invalid application JSON".into())
}
async fn write_frame<W: AsyncWrite + Unpin>(writer: &mut W, value: &Value) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    if bytes.is_empty() || bytes.len() > MAX_FRAME {
        return Err("Application frame exceeds limit".into());
    }
    tokio::time::timeout(IO_DEADLINE, async {
        writer.write_u32(bytes.len() as u32).await?;
        writer.write_all(&bytes).await?;
        writer.flush().await
    })
    .await
    .map_err(|_| "Application send stalled")?
    .map_err(|e: std::io::Error| e.to_string())
}

#[cfg(test)]
#[path = "tls_transport_tests.rs"]
mod tests;
