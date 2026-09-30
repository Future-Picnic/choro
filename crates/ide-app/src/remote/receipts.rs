use axum::{
    body::{to_bytes, Body},
    extract::Request,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use ide_core::local_store::{LocalStore, RemoteReceipt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

#[derive(Serialize, Deserialize)]
struct SavedResponse {
    status: u16,
    content_type: String,
    body: String,
}

fn error(status: u16, message: &str) -> Response {
    (
        axum::http::StatusCode::from_u16(status).unwrap(),
        Json(json!({"error": message})),
    )
        .into_response()
}

pub async fn durable_command(request: Request, next: Next) -> Response {
    if request.method() != axum::http::Method::POST {
        return next.run(request).await;
    }
    let path = request.uri().path();
    if !(path == "/v1/agents"
        || path.starts_with("/v1/agents/")
        || path == "/v1/remote/actions"
        || path == "/v1/remote/uploads")
    {
        return next.run(request).await;
    }
    let Some(device) = request.extensions().get::<super::PairedDevice>().cloned() else {
        return error(401, "Pair this device first");
    };
    let (parts, body) = request.into_parts();
    let bytes = match to_bytes(body, 6 * 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => return error(413, "Request is too large"),
    };
    let payload: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    let command = payload
        .get("client_command_id")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    if command.is_empty() {
        if parts.uri.path().starts_with("/v1/remote/") {
            return error(400, "A command identity is required");
        }
        return next
            .run(Request::from_parts(parts, Body::from(bytes)))
            .await;
    }
    let fingerprint = format!(
        "{:x}",
        Sha256::digest(format!("{}:{}", parts.uri, payload).as_bytes())
    );
    let key = (device.id, command);
    let reserved_key = key.clone();
    let reserved = tokio::task::spawn_blocking(move || {
        LocalStore::open_default()?.reserve_remote_command(
            &reserved_key.0,
            &reserved_key.1,
            &fingerprint,
        )
    })
    .await;
    match reserved {
        Ok(Ok(RemoteReceipt::New)) => {},
        Ok(Ok(RemoteReceipt::Complete(value))) => {
            let Ok(saved) = serde_json::from_str::<SavedResponse>(&value) else { return error(500, "Saved result could not be read"); };
            return (axum::http::StatusCode::from_u16(saved.status).unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR), [(axum::http::header::CONTENT_TYPE, saved.content_type)], BASE64.decode(saved.body).unwrap_or_default()).into_response();
        }
        Ok(Ok(RemoteReceipt::Pending)) => return error(409, "This command is still pending or was interrupted. Refresh the conversation before retrying; do not submit it again with a new identity."),
        Ok(Ok(RemoteReceipt::Conflict)) => return error(409, "This command identity was already used for a different request"),
        _ => return error(503, "Could not save this command. Nothing was started."),
    }
    let response = next
        .run(Request::from_parts(parts, Body::from(bytes)))
        .await;
    let (parts, body) = response.into_parts();
    let bytes = match to_bytes(body, 16 * 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => {
            return error(
                500,
                "The command result was too large. Refresh before retrying.",
            )
        }
    };
    let saved = SavedResponse {
        status: parts.status.as_u16(),
        content_type: parts
            .headers
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/json")
            .into(),
        body: BASE64.encode(&bytes),
    };
    let value = serde_json::to_string(&saved).unwrap();
    let stored = tokio::task::spawn_blocking(move || {
        LocalStore::open_default()?.complete_remote_command(&key.0, &key.1, &value)
    })
    .await;
    if !matches!(stored, Ok(Ok(()))) {
        return error(503, "The action may have completed, but its result could not be saved. Refresh before retrying.");
    }
    Response::from_parts(parts, Body::from(bytes))
}
