//! Shared request policy for legacy and TLS relay sessions.
use super::DevicePermission;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::net::SocketAddr;
use std::time::Duration;

const MAX_PATH_BYTES: usize = 2_048;
const MAX_BODY_BYTES: usize = 6 * 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

pub(super) fn forwarding_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()
        .map_err(|error| error.to_string())
}

pub(super) fn loopback_base(address: SocketAddr) -> Result<String, String> {
    if !address.ip().is_loopback() {
        return Err("Remote forwarding requires a loopback API".into());
    }
    Ok(format!("http://{address}"))
}

#[derive(Deserialize)]
pub(super) struct TunnelRequest {
    pub(super) request_id: String,
    pub(super) method: String,
    pub(super) path: String,
    pub(super) body: Option<String>,
}

#[derive(Serialize)]
pub(super) struct TunnelResponse {
    pub(super) request_id: String,
    pub(super) status: u16,
    pub(super) content_type: String,
    pub(super) body_base64: String,
}

pub(super) async fn forward_request(
    client: &reqwest::Client,
    local_base: &str,
    request: &TunnelRequest,
    token: &str,
) -> TunnelResponse {
    // Validate the destination before attaching the device bearer credential.
    let local = reqwest::Url::parse(local_base).ok();
    if !valid_request(request)
        || !local.is_some_and(|url| {
            url.scheme() == "http"
                && url.username().is_empty()
                && url.password().is_none()
                && url.path() == "/"
                && url.query().is_none()
                && url.fragment().is_none()
                && url
                    .host_str()
                    .and_then(|host| {
                        host.trim_matches(['[', ']'])
                            .parse::<std::net::IpAddr>()
                            .ok()
                    })
                    .is_some_and(|ip| ip.is_loopback())
        })
    {
        return json_response(
            &request.request_id,
            400,
            json!({"error":"Invalid remote destination or request"}),
        );
    }
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
        Ok(mut response) => {
            let status = response.status().as_u16();
            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or("application/octet-stream")
                .to_string();
            let bytes = async {
                let mut bytes = Vec::new();
                if response
                    .content_length()
                    .is_some_and(|size| size > MAX_RESPONSE_BYTES as u64)
                {
                    return Err("Remote response is too large".to_string());
                }
                while let Some(chunk) = response.chunk().await.map_err(|error| error.to_string())? {
                    if bytes.len() + chunk.len() > MAX_RESPONSE_BYTES {
                        return Err("Remote response is too large".to_string());
                    }
                    bytes.extend_from_slice(&chunk);
                }
                Ok(bytes)
            }
            .await;
            match bytes {
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
            json!({ "error": if request.method == "POST" {
                "The Desktop response was lost. This action may have completed. Check its status before trying again."
            } else { "Choro Desktop API is unavailable" } }),
        ),
    }
}

pub(super) fn valid_request(request: &TunnelRequest) -> bool {
    let route = request.path.split('?').next().unwrap_or_default();
    matches!(request.method.as_str(), "GET" | "POST")
        && route.starts_with("/v1/")
        && !route.contains(['%', '\\', '#'])
        && !route.contains("//")
        && !request.path.chars().any(char::is_control)
        && !request.path.contains('#')
        && allowlisted_route(&request.method, &request.path)
        && !request
            .path
            .split('?')
            .next()
            .unwrap_or_default()
            .contains("..")
        && request.path.len() <= MAX_PATH_BYTES
        && request.body.as_ref().map_or(0, String::len) <= MAX_BODY_BYTES
        && (1..=128).contains(&request.request_id.len())
}

pub(super) fn allowlisted_route(method: &str, path: &str) -> bool {
    let route = path.split('?').next().unwrap_or_default();
    let segments = route.trim_matches('/').split('/').collect::<Vec<_>>();
    match method {
        "GET" => matches!(
            segments.as_slice(),
            ["v1", "health"]
                | ["v1", "configuration"]
                | ["v1", "device"]
                | ["v1", "capabilities"]
                | ["v1", "workspace", "agents"]
                | [
                    "v1",
                    "remote",
                    "agent"
                        | "search"
                        | "summary"
                        | "delegation"
                        | "diff"
                        | "bandmates"
                        | "attachment"
                ]
                | ["v1", "projects"]
                | ["v1", "projects", _, "agents"]
                | ["v1", "agents", _]
                | ["v1", "agents", _, "visualizations", "snapshot"]
                | ["v1", "agents", _, "images", "preview"]
                | ["v1", "agents", _, "diff"]
                | ["v1", "agents", _, "ship", "preview"]
        ),
        "POST" => matches!(
            segments.as_slice(),
            ["v1", "agents"]
                | ["v1", "remote", "actions"]
                | ["v1", "remote", "uploads"]
                | ["v1", "agents", _, "verification", "fix"]
                | ["v1", "agents", _, "ship"]
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

pub(super) fn authorization_error(
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
    if request.path.contains("/approvals/")
        || request
            .path
            .split('?')
            .next()
            .is_some_and(|p| p.ends_with("/ship"))
    {
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

pub(super) fn json_response(
    request_id: &str,
    status: u16,
    value: serde_json::Value,
) -> TunnelResponse {
    TunnelResponse {
        request_id: request_id.to_string(),
        status,
        content_type: "application/json".into(),
        body_base64: BASE64.encode(serde_json::to_vec(&value).unwrap_or_default()),
    }
}
