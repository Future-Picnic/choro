use super::*;

pub(super) fn serde_label<T: Serialize>(value: &T) -> Result<String> {
    match serde_json::to_value(value)? {
        serde_json::Value::String(value) => Ok(value),
        other => Err(anyhow!(
            "expected enum to serialize as string, got {other:?}"
        )),
    }
}

pub(super) fn serde_parse<T: DeserializeOwned>(value: &str) -> Result<T> {
    serde_json::from_value(serde_json::Value::String(value.to_string())).map_err(Into::into)
}

pub(super) fn parse_uuid(value: &str) -> Result<Uuid> {
    Uuid::parse_str(value).with_context(|| format!("invalid uuid {value}"))
}

pub(super) fn bool_to_i64(value: bool) -> i64 {
    if value {
        1
    } else {
        0
    }
}

pub(super) fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

pub(super) fn opt_path_to_string(path: Option<&Path>) -> Option<String> {
    path.map(path_to_string)
}

pub(super) fn u64_to_i64(value: u64) -> Result<i64> {
    i64::try_from(value).context("value does not fit into SQLite INTEGER")
}

pub(super) fn i64_to_u64(value: i64) -> Result<u64> {
    u64::try_from(value).context("negative value where unsigned value was expected")
}

pub(super) fn i64_to_usize(value: i64) -> Result<usize> {
    usize::try_from(value).context("value does not fit into usize")
}

pub(super) fn opt_text(row: &turso::Row, idx: usize) -> Result<Option<String>> {
    match row.get_value(idx)? {
        Value::Null => Ok(None),
        Value::Text(text) => Ok(Some(text)),
        other => Err(anyhow!(
            "expected nullable text at column {idx}, got {other:?}"
        )),
    }
}

pub(super) fn opt_i64(row: &turso::Row, idx: usize) -> Result<Option<i64>> {
    match row.get_value(idx)? {
        Value::Null => Ok(None),
        Value::Integer(value) => Ok(Some(value)),
        other => Err(anyhow!(
            "expected nullable integer at column {idx}, got {other:?}"
        )),
    }
}

pub(super) fn should_keep_existing_message_payload(
    kind: &str,
    event_key: Option<&str>,
    existing_payload_json: &str,
    incoming_payload_json: &str,
) -> bool {
    if kind != "message"
        || !event_key.is_some_and(|key| key.starts_with("message:") && key.contains(":backend:"))
    {
        return false;
    }

    let Some(existing_text) = message_payload_text(existing_payload_json) else {
        return false;
    };
    let Some(incoming_text) = message_payload_text(incoming_payload_json) else {
        return false;
    };
    should_keep_existing_stream_text(&existing_text, &incoming_text)
}

pub(super) fn message_payload_text(payload_json: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(payload_json)
        .ok()?
        .get("text")?
        .as_str()
        .map(str::to_string)
}

pub(super) fn should_keep_existing_stream_text(existing: &str, incoming: &str) -> bool {
    if incoming.is_empty() {
        return !existing.is_empty();
    }
    existing.len() > incoming.len()
        && (existing.starts_with(incoming)
            || existing.ends_with(incoming)
            || existing.contains(incoming))
}

pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

pub(super) fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
