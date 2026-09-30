use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use ide_core::preview_control::{
    read_frame, write_frame, PreviewControlEndpoint, PreviewControlRequest, PreviewControlResponse,
    PREVIEW_CONTROL_DESCRIPTOR_NAME, PREVIEW_CONTROL_MAX_REQUEST_BYTES,
    PREVIEW_CONTROL_MAX_RESPONSE_BYTES, PREVIEW_CONTROL_PROTOCOL_VERSION,
    PREVIEW_CONTROL_SOCKET_NAME, PREVIEW_CONTROL_TIMEOUT,
};
use uuid::Uuid;

const MAX_CONNECTIONS: usize = 64;

pub(super) struct PreviewControlEnvelope {
    pub request: PreviewControlRequest,
    pub respond_to: mpsc::SyncSender<PreviewControlResponse>,
}

pub(super) struct PreviewControlServer {
    socket_path: PathBuf,
    descriptor_path: PathBuf,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl PreviewControlServer {
    pub fn start(root: PathBuf) -> Result<(Self, async_channel::Receiver<PreviewControlEnvelope>)> {
        #[cfg(not(unix))]
        {
            let _ = root;
            return Err(anyhow!("Preview control requires Unix-domain sockets"));
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            use std::os::unix::net::{UnixListener, UnixStream};

            fs::create_dir_all(&root)?;
            let socket_path = root.join(PREVIEW_CONTROL_SOCKET_NAME);
            let descriptor_path = root.join(PREVIEW_CONTROL_DESCRIPTOR_NAME);
            if socket_path.exists() {
                if UnixStream::connect(&socket_path).is_ok() {
                    return Err(anyhow!(
                        "another Choro process already owns Preview control"
                    ));
                }
                fs::remove_file(&socket_path)
                    .with_context(|| "could not remove stale Preview control socket")?;
            }

            let listener = UnixListener::bind(&socket_path)
                .with_context(|| "could not start local Preview control")?;
            fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))?;
            listener.set_nonblocking(true)?;

            let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
            let endpoint = PreviewControlEndpoint {
                version: PREVIEW_CONTROL_PROTOCOL_VERSION,
                pid: std::process::id(),
                socket_path: socket_path.clone(),
                token: token.clone(),
            };
            let descriptor_bytes = serde_json::to_vec(&endpoint)?;
            let temporary_descriptor = root.join(format!(
                ".{PREVIEW_CONTROL_DESCRIPTOR_NAME}.{}",
                std::process::id()
            ));
            fs::write(&temporary_descriptor, descriptor_bytes)?;
            fs::set_permissions(&temporary_descriptor, fs::Permissions::from_mode(0o600))?;
            fs::rename(&temporary_descriptor, &descriptor_path)?;

            let (sender, receiver) = async_channel::bounded(MAX_CONNECTIONS);
            let stop = Arc::new(AtomicBool::new(false));
            let server_stop = stop.clone();
            let active_connections = Arc::new(AtomicUsize::new(0));
            let thread = thread::Builder::new()
                .name("preview-control-ipc".to_string())
                .spawn(move || {
                    while !server_stop.load(Ordering::Acquire) {
                        match listener.accept() {
                            Ok((stream, _)) => {
                                if active_connections.fetch_add(1, Ordering::AcqRel)
                                    >= MAX_CONNECTIONS
                                {
                                    active_connections.fetch_sub(1, Ordering::AcqRel);
                                    continue;
                                }
                                let sender = sender.clone();
                                let token = token.clone();
                                let active_connections = active_connections.clone();
                                let _ = thread::Builder::new()
                                    .name("preview-control-request".to_string())
                                    .spawn(move || {
                                        handle_connection(stream, &token, &sender);
                                        active_connections.fetch_sub(1, Ordering::AcqRel);
                                    });
                            }
                            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                                thread::sleep(Duration::from_millis(5));
                            }
                            Err(error) => {
                                eprintln!("Preview control listener failed: {error}");
                                thread::sleep(Duration::from_millis(25));
                            }
                        }
                    }
                })?;

            Ok((
                Self {
                    socket_path,
                    descriptor_path,
                    stop,
                    thread: Some(thread),
                },
                receiver,
            ))
        }
    }
}

#[cfg(unix)]
fn handle_connection(
    mut stream: std::os::unix::net::UnixStream,
    token: &str,
    sender: &async_channel::Sender<PreviewControlEnvelope>,
) {
    // Accepted sockets inherit O_NONBLOCK from the listener on macOS. Put the
    // per-request stream back into blocking mode so large PNG responses are
    // fully drained instead of ending after the first kernel socket buffer.
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
    let request = read_frame(&mut stream, PREVIEW_CONTROL_MAX_REQUEST_BYTES).and_then(|bytes| {
        serde_json::from_slice::<PreviewControlRequest>(&bytes).map_err(Into::into)
    });
    let response = match request {
        Ok(request)
            if request.version == PREVIEW_CONTROL_PROTOCOL_VERSION
                && constant_time_eq(request.token.as_bytes(), token.as_bytes()) =>
        {
            let request_id = request.id;
            let (respond_to, response) = mpsc::sync_channel(1);
            match sender.send_blocking(PreviewControlEnvelope {
                request,
                respond_to,
            }) {
                Ok(()) => response
                    .recv_timeout(PREVIEW_CONTROL_TIMEOUT)
                    .unwrap_or_else(|_| {
                        PreviewControlResponse::failure(
                            request_id,
                            "Preview action expired before the UI completed it",
                        )
                    }),
                Err(_) => PreviewControlResponse::failure(
                    request_id,
                    "Choro Preview control is shutting down",
                ),
            }
        }
        Ok(request) => PreviewControlResponse::failure(
            request.id,
            "Preview control authentication failed; restart Choro and this agent",
        ),
        Err(error) => PreviewControlResponse::failure(
            Uuid::nil(),
            format!("Invalid Preview control request: {error:#}"),
        ),
    };
    let mut bytes = serde_json::to_vec(&response).unwrap_or_default();
    if bytes.len() > PREVIEW_CONTROL_MAX_RESPONSE_BYTES {
        bytes = serde_json::to_vec(&PreviewControlResponse::failure(
            response.id,
            "Preview response exceeded the 8 MB transport limit",
        ))
        .unwrap_or_default();
    }
    if let Err(error) = write_frame(&mut stream, &bytes, PREVIEW_CONTROL_MAX_RESPONSE_BYTES) {
        eprintln!("could not return Preview control response: {error:#}");
    }
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

impl Drop for PreviewControlServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = fs::remove_file(&self.socket_path);
        let descriptor_matches_process = fs::read(&self.descriptor_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<PreviewControlEndpoint>(&bytes).ok())
            .is_some_and(|endpoint| endpoint.pid == std::process::id());
        if descriptor_matches_process {
            let _ = fs::remove_file(&self.descriptor_path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_comparison_rejects_size_and_content_changes() {
        assert!(constant_time_eq(b"secret", b"secret"));
        assert!(!constant_time_eq(b"secret", b"Secret"));
        assert!(!constant_time_eq(b"secret", b"secret!"));
    }

    #[cfg(unix)]
    #[test]
    fn socket_server_delivers_and_completes_a_scoped_request() {
        let directory = tempfile::tempdir().unwrap();
        let (_server, receiver) =
            PreviewControlServer::start(directory.path().to_path_buf()).unwrap();
        let root = directory.path().to_path_buf();
        let project_id = ide_core::ProjectId(Uuid::from_u128(7));
        let agent_id = Uuid::from_u128(8);
        let client = thread::spawn(move || {
            ide_core::preview_control::call_preview_control(
                &root,
                project_id,
                agent_id,
                "click",
                r#"{"ref":"e1"}"#.to_string(),
            )
            .unwrap()
        });

        let envelope = receiver.recv_blocking().unwrap();
        assert_eq!(envelope.request.project_id, project_id);
        assert_eq!(envelope.request.agent_id, agent_id);
        assert_eq!(envelope.request.action, "click");
        let image_base64 = "A".repeat(512 * 1024);
        envelope
            .respond_to
            .send(PreviewControlResponse::success(
                envelope.request.id,
                r#"{"ok":true}"#.to_string(),
                Some(image_base64.clone()),
            ))
            .unwrap();

        let response = client.join().unwrap();
        assert!(response.ok);
        assert_eq!(response.result_json.as_deref(), Some(r#"{"ok":true}"#));
        assert_eq!(
            response.image_base64.as_deref(),
            Some(image_base64.as_str())
        );
    }
}
