//! Synthetic integration tests for the production TLS framing/application code.
//! All certificates, bearer credentials, data files and HTTP servers are local
//! fixtures. Nothing opens Choro's live storage or Keychain.
use super::*;
use crate::remote::{auth::PairingResult, DevicePermission};
use rustls::{
    pki_types::{PrivatePkcs8KeyDer, ServerName},
    ClientConfig, RootCertStore, ServerConfig,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio_rustls::{client::TlsStream, TlsConnector};

struct Session {
    tls: TlsStream<tokio::io::DuplexStream>,
    server: tokio::task::JoinHandle<Result<(), String>>,
    bridge: tokio::task::JoinHandle<()>,
    events: broadcast::Sender<RemoteEvent>,
    _admitted: mpsc::Receiver<(Uuid, String)>,
}

impl Drop for Session {
    fn drop(&mut self) {
        self.server.abort();
        self.bridge.abort();
    }
}

fn configurations() -> (TlsAcceptor, TlsConnector) {
    let mut params = rcgen::CertificateParams::default();
    params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Constrained(0));
    params.key_usages = vec![rcgen::KeyUsagePurpose::KeyCertSign];
    let ca = rcgen::CertifiedIssuer::self_signed(
        params,
        rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap(),
    )
    .unwrap();
    let mut params = rcgen::CertificateParams::new(vec!["desktop.choro.invalid".into()]).unwrap();
    params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
    let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
    let leaf = params.signed_by(&key, &ca).unwrap();
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut server = ServerConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![leaf.der().clone()],
            PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
        )
        .unwrap();
    server.alpn_protocols = vec![b"choro-remote/3".to_vec()];
    server.send_tls13_tickets = 0;
    let mut roots = RootCertStore::empty();
    roots.add(ca.der().clone()).unwrap();
    let mut client = ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
    client.alpn_protocols = server.alpn_protocols.clone();
    (
        TlsAcceptor::from(Arc::new(server)),
        TlsConnector::from(Arc::new(client)),
    )
}

async fn session(auth: RemoteAuth, base: String, pairing: bool) -> Session {
    let (acceptor, connector) = configurations();
    let id = Uuid::new_v4();
    let (input_tx, input_rx) = mpsc::channel(4);
    let (output_tx, mut output_rx) = mpsc::channel::<Message>(4);
    let (events, receiver) = broadcast::channel(16);
    let (admitted, admitted_rx) = mpsc::channel(4);
    let server = tokio::spawn(run_stream(
        id,
        input_rx,
        output_tx,
        acceptor,
        auth,
        receiver,
        forwarding_client().unwrap(),
        base,
        pairing,
        admitted,
    ));
    let (client, bridge) = tokio::io::duplex(CHUNK);
    let (mut bridge_read, mut bridge_write) = tokio::io::split(bridge);
    let bridge = tokio::spawn(async move {
        let inward = async {
            let mut buffer = vec![0; 4096];
            loop {
                let size = bridge_read.read(&mut buffer).await.unwrap();
                if size == 0 {
                    break;
                }
                // Split encrypted TLS records across relay chunks, including
                // record headers. Subsequent chunks may combine TLS records.
                for bytes in buffer[..size].chunks(317) {
                    if input_tx.send(bytes.to_vec()).await.is_err() {
                        return;
                    }
                }
            }
        };
        let outward = async {
            while let Some(Message::Binary(bytes)) = output_rx.recv().await {
                assert_eq!(&bytes[..16], id.as_bytes());
                if bridge_write.write_all(&bytes[16..]).await.is_err() {
                    return;
                }
            }
        };
        tokio::select! { _ = inward => {}, _ = outward => {} }
    });
    let tls = tokio::time::timeout(
        Duration::from_secs(5),
        connector.connect(
            ServerName::try_from("desktop.choro.invalid").unwrap(),
            client,
        ),
    )
    .await
    .unwrap()
    .unwrap();
    Session {
        tls,
        server,
        bridge,
        events,
        _admitted: admitted_rx,
    }
}

fn paired(auth: &RemoteAuth) -> PairingResult {
    let code = auth.start_pairing().active_code.unwrap();
    auth.pair(&code, "Synthetic test iPhone").unwrap()
}

async fn receive(session: &mut Session) -> Value {
    tokio::time::timeout(Duration::from_secs(3), read_frame(&mut session.tls))
        .await
        .unwrap()
        .unwrap()
}

async fn authenticate(session: &mut Session, pair: &PairingResult) {
    write_frame(&mut session.tls, &json!({"type":"authenticate","request_id":"auth","device_id":pair.device.id,"token":pair.token})).await.unwrap();
    assert_eq!(receive(session).await["type"], "authenticated");
}

struct LocalApi {
    base: String,
    reads: Arc<AtomicUsize>,
    writes: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for LocalApi {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn local_api(delay_write_response: bool) -> LocalApi {
    let reads = Arc::new(AtomicUsize::new(0));
    let writes = Arc::new(AtomicUsize::new(0));
    let read_count = reads.clone();
    let write_count = writes.clone();
    let router = axum::Router::new().fallback(
        move |method: axum::http::Method, headers: axum::http::HeaderMap| {
            let reads = read_count.clone();
            let writes = write_count.clone();
            async move {
                assert!(headers
                    .get("authorization")
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .starts_with("Bearer choro_device_"));
                if method == axum::http::Method::POST {
                    writes.fetch_add(1, Ordering::SeqCst);
                    if delay_write_response {
                        tokio::time::sleep(Duration::from_secs(2)).await;
                    }
                } else {
                    reads.fetch_add(1, Ordering::SeqCst);
                }
                axum::Json(json!({"synthetic":true}))
            }
        },
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    LocalApi {
        base,
        reads,
        writes,
        task,
    }
}

#[tokio::test]
async fn framing_accepts_fragmented_prefix_and_combined_messages_and_rejects_bad_lengths() {
    let (mut tx, mut rx) = tokio::io::duplex(64);
    let first = json!({"request_id":"one","value":"fragmented"});
    let second = json!({"request_id":"two","value":"coalesced"});
    let expected = (first.clone(), second.clone());
    let writer = tokio::spawn(async move {
        let bytes = serde_json::to_vec(&first).unwrap();
        for byte in (bytes.len() as u32).to_be_bytes() {
            tx.write_all(&[byte]).await.unwrap();
            tokio::task::yield_now().await;
        }
        tx.write_all(&bytes).await.unwrap();
        let bytes = serde_json::to_vec(&second).unwrap();
        let mut combined = (bytes.len() as u32).to_be_bytes().to_vec();
        combined.extend(bytes);
        tx.write_all(&combined).await.unwrap();
    });
    assert_eq!(read_frame(&mut rx).await.unwrap(), expected.0);
    assert_eq!(read_frame(&mut rx).await.unwrap(), expected.1);
    writer.await.unwrap();
    for size in [0, MAX_FRAME as u32 + 1, u32::MAX] {
        let mut prefix = std::io::Cursor::new(size.to_be_bytes());
        assert!(read_frame(&mut prefix)
            .await
            .unwrap_err()
            .contains("Oversized or empty"));
    }
}

#[tokio::test]
async fn real_tls_rejects_requests_before_device_authentication() {
    let directory = tempfile::tempdir().unwrap();
    let auth = RemoteAuth::load(directory.path().join("auth.json"));
    let api = local_api(false).await;
    let mut connection = session(auth, api.base.clone(), false).await;
    write_frame(&mut connection.tls, &json!({"type":"request","request_id":"unauthorized","method":"POST","path":"/v1/agents","body":"{}"})).await.unwrap();
    let error = tokio::time::timeout(Duration::from_secs(2), &mut connection.server)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert!(error.contains("Unauthorized"));
    assert_eq!(api.writes.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn real_tls_rechecks_permissions_and_revocation_on_an_active_session() {
    let directory = tempfile::tempdir().unwrap();
    let auth = RemoteAuth::load(directory.path().join("auth.json"));
    let pair = paired(&auth);
    auth.set_device_permission(&pair.device.id, DevicePermission::ViewOnly)
        .unwrap();
    let api = local_api(false).await;
    let mut connection = session(auth.clone(), api.base.clone(), false).await;
    authenticate(&mut connection, &pair).await;
    for (id, method, path, expected) in [
        ("read", "GET", "/v1/projects", 200),
        ("blocked", "POST", "/v1/agents", 403),
    ] {
        write_frame(
            &mut connection.tls,
            &json!({"type":"request","request_id":id,"method":method,"path":path,"body":"{}"}),
        )
        .await
        .unwrap();
        assert_eq!(receive(&mut connection).await["status"], expected);
    }
    auth.set_device_permission(&pair.device.id, DevicePermission::Control)
        .unwrap();
    write_frame(&mut connection.tls, &json!({"type":"request","request_id":"control","method":"POST","path":"/v1/agents","body":"{}"})).await.unwrap();
    assert_eq!(receive(&mut connection).await["status"], 200);
    write_frame(&mut connection.tls, &json!({"type":"request","request_id":"approval-denied","method":"POST","path":"/v1/agents/task/approvals/prompt/resolve","body":"{}"})).await.unwrap();
    assert_eq!(receive(&mut connection).await["status"], 403);
    auth.set_device_permission(&pair.device.id, DevicePermission::FullAccess)
        .unwrap();
    write_frame(&mut connection.tls, &json!({"type":"request","request_id":"approval-allowed","method":"POST","path":"/v1/agents/task/approvals/prompt/resolve","body":"{}"})).await.unwrap();
    assert_eq!(receive(&mut connection).await["status"], 200);
    connection
        .events
        .send(RemoteEvent::HostSnapshotChanged)
        .unwrap();
    assert_eq!(
        receive(&mut connection).await["event"]["type"],
        "host_snapshot_changed"
    );
    auth.revoke(&pair.device.id).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(2), &mut connection.server)
            .await
            .unwrap()
            .unwrap()
            .is_err()
    );
    assert_eq!(api.reads.load(Ordering::SeqCst), 1);
    assert_eq!(api.writes.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn real_tls_pairing_commit_recovers_after_restart_and_lost_response() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("auth.json");
    let auth = RemoteAuth::load(path.clone());
    let old = paired(&auth);
    let code = auth.start_pairing().active_code.unwrap();
    let mut connection = session(auth.clone(), "http://127.0.0.1:1".into(), true).await;
    write_frame(&mut connection.tls, &json!({"type":"pair","request_id":"prepare","code":code,"device_name":"Migrated iPhone","previous_token":old.token})).await.unwrap();
    let prepared = receive(&mut connection).await;
    assert_eq!(prepared["type"], "paired");
    assert!(auth.authorize(&old.token));
    assert!(!auth.authorize(prepared["token"].as_str().unwrap()));
    drop(connection);
    let auth = RemoteAuth::load(path.clone());
    let mut recovery = session(auth.clone(), "http://127.0.0.1:1".into(), false).await;
    let commit = json!({"type":"pair_commit","request_id":"commit","transaction_id":prepared["transaction_id"],"token":prepared["token"]});
    write_frame(&mut recovery.tls, &commit).await.unwrap();
    assert_eq!(receive(&mut recovery).await["type"], "pair_committed");
    drop(recovery);
    let reloaded = RemoteAuth::load(path);
    let mut recovery = session(reloaded.clone(), "http://127.0.0.1:1".into(), false).await;
    write_frame(&mut recovery.tls, &commit).await.unwrap();
    assert_eq!(
        receive(&mut recovery).await["device_id"],
        prepared["device"]["id"]
    );
    assert!(!reloaded.authorize(&old.token));
    assert!(reloaded.authorize(prepared["token"].as_str().unwrap()));
    assert_eq!(reloaded.snapshot().devices.len(), 1);
}

#[tokio::test]
async fn disconnect_after_a_write_never_replays_it_on_a_fresh_session() {
    let directory = tempfile::tempdir().unwrap();
    let auth = RemoteAuth::load(directory.path().join("auth.json"));
    let pair = paired(&auth);
    let api = local_api(true).await;
    let mut connection = session(auth.clone(), api.base.clone(), false).await;
    authenticate(&mut connection, &pair).await;
    write_frame(&mut connection.tls, &json!({"type":"request","request_id":"uncertain-write","method":"POST","path":"/v1/agents","body":"{}"})).await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while api.writes.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    drop(connection); // Server committed a write; phone did not receive response.
    let mut reconnected = session(auth, api.base.clone(), false).await;
    authenticate(&mut reconnected, &pair).await;
    write_frame(&mut reconnected.tls, &json!({"type":"request","request_id":"read-after-reconnect","method":"GET","path":"/v1/projects"})).await.unwrap();
    assert_eq!(receive(&mut reconnected).await["status"], 200);
    assert_eq!(api.writes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn large_tls_transfers_succeed_and_a_nonreading_peer_is_closed() {
    let directory = tempfile::tempdir().unwrap();
    let auth = RemoteAuth::load(directory.path().join("auth.json"));
    let pair = paired(&auth);
    let payload = "synthetic-attachment-".repeat(110_000);
    let download = payload.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let router = axum::Router::new()
        .route(
            "/v1/agents",
            axum::routing::post(|body: String| async move { body }),
        )
        .route(
            "/v1/projects",
            axum::routing::get(move || {
                let payload = download.clone();
                async move { payload }
            }),
        )
        .layer(axum::extract::DefaultBodyLimit::max(6 * 1024 * 1024));
    let api = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let mut connection = session(auth.clone(), base.clone(), false).await;
    authenticate(&mut connection, &pair).await;
    write_frame(&mut connection.tls, &json!({"type":"request","request_id":"large-upload","method":"POST","path":"/v1/agents","body":payload})).await.unwrap();
    let response = receive(&mut connection).await;
    assert_eq!(response["status"], 200);
    assert_eq!(
        STANDARD
            .decode(response["body_base64"].as_str().unwrap())
            .unwrap(),
        payload.as_bytes()
    );
    drop(connection);

    let mut stalled = session(auth, base, false).await;
    authenticate(&mut stalled, &pair).await;
    write_frame(&mut stalled.tls, &json!({"type":"request","request_id":"nonreading-download","method":"GET","path":"/v1/projects"})).await.unwrap();
    // Deliberately stop reading. The real TLS -> relay output queues fill and
    // terminate this stream within their deadline rather than growing forever.
    let result = tokio::time::timeout(Duration::from_secs(8), &mut stalled.server)
        .await
        .unwrap()
        .unwrap();
    assert!(result.unwrap_err().contains("stalled"));
    api.abort();
}

#[tokio::test]
async fn forwarding_never_follows_redirects_or_sends_credentials_to_external_destinations() {
    let target_hits = Arc::new(AtomicUsize::new(0));
    let hits = target_hits.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = axum::Router::new()
        .route(
            "/v1/projects",
            axum::routing::get(|| async {
                axum::response::Redirect::temporary("/credential-collector")
            }),
        )
        .route(
            "/credential-collector",
            axum::routing::get(move || {
                let hits = hits.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    "unexpected"
                }
            }),
        );
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let client = forwarding_client().unwrap();
    let request = TunnelRequest {
        request_id: "redirect".into(),
        method: "GET".into(),
        path: "/v1/projects".into(),
        body: None,
    };
    let response = forward_request(
        &client,
        &format!("http://{address}"),
        &request,
        "choro_device_fixture",
    )
    .await;
    assert_eq!(response.status, 307);
    assert_eq!(target_hits.load(Ordering::SeqCst), 0);
    for base in [
        "http://example.com",
        "http://127.0.0.1.evil.invalid",
        "http://user:pass@127.0.0.1",
        "https://127.0.0.1",
        "http://127.0.0.1/path",
    ] {
        assert_eq!(
            forward_request(&client, base, &request, "choro_device_fixture")
                .await
                .status,
            400
        );
    }
    server.abort();
}
