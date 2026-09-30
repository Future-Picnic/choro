//! Authenticated, local-only transport for controlling Choro's live Preview.
//!
//! Preview commands are latency-sensitive and must not contend with the
//! embedded workspace database. The desktop app publishes a short-lived
//! capability token and Unix-domain socket; the agent-side MCP process sends
//! one length-prefixed request and receives one bounded response.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::project::ProjectId;

pub const PREVIEW_CONTROL_PROTOCOL_VERSION: u32 = 1;
pub const PREVIEW_CONTROL_DESCRIPTOR_NAME: &str = "preview-control-v1.json";
pub const PREVIEW_CONTROL_SOCKET_NAME: &str = "preview-control-v1.sock";
pub const PREVIEW_CONTROL_MAX_REQUEST_BYTES: usize = 64 * 1024;
pub const PREVIEW_CONTROL_MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
pub const PREVIEW_CONTROL_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PreviewControlEndpoint {
    pub version: u32,
    pub pid: u32,
    pub socket_path: PathBuf,
    pub token: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PreviewControlRequest {
    pub version: u32,
    pub id: Uuid,
    pub token: String,
    pub project_id: ProjectId,
    pub agent_id: Uuid,
    pub action: String,
    pub payload_json: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PreviewControlResponse {
    pub version: u32,
    pub id: Uuid,
    pub ok: bool,
    pub result_json: Option<String>,
    pub image_base64: Option<String>,
    pub error: Option<String>,
}

impl PreviewControlResponse {
    pub fn success(id: Uuid, result_json: String, image_base64: Option<String>) -> Self {
        Self {
            version: PREVIEW_CONTROL_PROTOCOL_VERSION,
            id,
            ok: true,
            result_json: Some(result_json),
            image_base64,
            error: None,
        }
    }

    pub fn failure(id: Uuid, error: impl Into<String>) -> Self {
        Self {
            version: PREVIEW_CONTROL_PROTOCOL_VERSION,
            id,
            ok: false,
            result_json: None,
            image_base64: None,
            error: Some(error.into()),
        }
    }
}

pub fn preview_control_descriptor_path(root: &Path) -> PathBuf {
    root.join(PREVIEW_CONTROL_DESCRIPTOR_NAME)
}

pub fn preview_control_socket_path(root: &Path) -> PathBuf {
    root.join(PREVIEW_CONTROL_SOCKET_NAME)
}

pub fn read_preview_control_endpoint(root: &Path) -> Result<PreviewControlEndpoint> {
    let path = preview_control_descriptor_path(root);
    let metadata = fs::metadata(&path)
        .with_context(|| "Choro Preview control is not available; keep the desktop app open")?;
    if metadata.len() > 16 * 1024 {
        return Err(anyhow!("Choro Preview control descriptor is invalid"));
    }
    let bytes = fs::read(&path)
        .with_context(|| "Choro Preview control is not available; keep the desktop app open")?;
    let endpoint: PreviewControlEndpoint =
        serde_json::from_slice(&bytes).context("Choro Preview control descriptor is invalid")?;
    if endpoint.version != PREVIEW_CONTROL_PROTOCOL_VERSION {
        return Err(anyhow!(
            "Choro Preview control protocol changed; restart Choro and this agent"
        ));
    }
    Ok(endpoint)
}

#[cfg(unix)]
pub fn call_preview_control(
    root: &Path,
    project_id: ProjectId,
    agent_id: Uuid,
    action: &str,
    payload_json: String,
) -> Result<PreviewControlResponse> {
    use std::os::unix::net::UnixStream;

    let endpoint = read_preview_control_endpoint(root)?;
    let request = PreviewControlRequest {
        version: PREVIEW_CONTROL_PROTOCOL_VERSION,
        id: Uuid::new_v4(),
        token: endpoint.token,
        project_id,
        agent_id,
        action: action.to_string(),
        payload_json,
    };
    let mut stream = UnixStream::connect(&endpoint.socket_path).with_context(|| {
        "Could not reach Choro Preview control; restart Choro if it was just updated"
    })?;
    stream.set_read_timeout(Some(PREVIEW_CONTROL_TIMEOUT))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    write_frame(
        &mut stream,
        &serde_json::to_vec(&request)?,
        PREVIEW_CONTROL_MAX_REQUEST_BYTES,
    )?;
    let bytes = read_frame(&mut stream, PREVIEW_CONTROL_MAX_RESPONSE_BYTES)?;
    let response: PreviewControlResponse =
        serde_json::from_slice(&bytes).context("Choro returned an invalid Preview response")?;
    if response.version != PREVIEW_CONTROL_PROTOCOL_VERSION || response.id != request.id {
        return Err(anyhow!("Choro returned a mismatched Preview response"));
    }
    Ok(response)
}

#[cfg(not(unix))]
pub fn call_preview_control(
    _root: &Path,
    _project_id: ProjectId,
    _agent_id: Uuid,
    _action: &str,
    _payload_json: String,
) -> Result<PreviewControlResponse> {
    Err(anyhow!(
        "Preview control is currently available on macOS only"
    ))
}

pub fn write_frame(writer: &mut impl Write, bytes: &[u8], maximum: usize) -> Result<()> {
    if bytes.len() > maximum || bytes.len() > u32::MAX as usize {
        return Err(anyhow!("Preview control message exceeds its size limit"));
    }
    writer.write_all(&(bytes.len() as u32).to_be_bytes())?;
    writer.write_all(bytes)?;
    writer.flush()?;
    Ok(())
}

pub fn read_frame(reader: &mut impl Read, maximum: usize) -> Result<Vec<u8>> {
    let mut length = [0_u8; 4];
    reader.read_exact(&mut length)?;
    let length = u32::from_be_bytes(length) as usize;
    if length > maximum {
        return Err(anyhow!("Preview control message exceeds its size limit"));
    }
    let mut bytes = vec![0_u8; length];
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framed_messages_round_trip_and_enforce_limits() {
        let mut bytes = Vec::new();
        write_frame(&mut bytes, b"preview", 32).unwrap();
        assert_eq!(read_frame(&mut bytes.as_slice(), 32).unwrap(), b"preview");
        assert!(write_frame(&mut Vec::new(), b"too long", 3).is_err());
        assert!(read_frame(&mut [0, 0, 0, 9].as_slice(), 8).is_err());
    }
}
