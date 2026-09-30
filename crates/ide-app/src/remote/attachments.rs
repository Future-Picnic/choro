//! Opaque device-owned uploads. No client-supplied filesystem paths and no cleanup.
use super::{PairedDevice, RemoteError, RemoteResult};
use axum::{
    extract::Extension,
    response::{IntoResponse, Response},
    Json,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};
use uuid::Uuid;

const MAX_IMAGE: usize = 4 * 1024 * 1024;
static UPLOAD_LOCK: Mutex<()> = Mutex::new(());

#[derive(Serialize, Deserialize)]
struct Manifest {
    device: String,
    name: String,
    size: usize,
    complete: bool,
    #[serde(default)]
    file_name: String,
}

fn storage_root() -> RemoteResult<PathBuf> {
    Ok(ide_core::local_store::LocalStore::open_default()
        .map_err(|e| RemoteError::internal(e.to_string()))?
        .root()
        .join("remote-attachments"))
}
fn folder(root: &Path, id: &str) -> RemoteResult<PathBuf> {
    let id =
        Uuid::parse_str(id).map_err(|_| RemoteError::bad_request("Invalid attachment identity"))?;
    Ok(root.join(id.to_string()))
}
fn manifest(root: &Path, id: &str, device: &str) -> RemoteResult<(PathBuf, Manifest)> {
    let dir = folder(root, id)?;
    let m: Manifest = serde_json::from_slice(
        &fs::read(dir.join("manifest.json"))
            .map_err(|_| RemoteError::not_found("Attachment not found"))?,
    )
    .map_err(|_| RemoteError::internal("Attachment metadata is unreadable"))?;
    if m.device != device {
        return Err(RemoteError::not_found("Attachment not found"));
    }
    Ok((dir, m))
}
pub fn resolve(ids: &[String], device: &str) -> RemoteResult<Vec<PathBuf>> {
    if ids.is_empty() {
        return Ok(vec![]);
    }
    resolve_at(&storage_root()?, ids, device)
}
fn resolve_at(root: &Path, ids: &[String], device: &str) -> RemoteResult<Vec<PathBuf>> {
    if ids.len() > 4 {
        return Err(RemoteError::bad_request("Attach up to four images"));
    }
    ids.iter()
        .map(|id| {
            let (dir, m) = manifest(root, id, device)?;
            if !m.complete {
                return Err(RemoteError::conflict(
                    "Finish uploading the image before sending",
                ));
            }
            Ok(dir.join(&m.file_name))
        })
        .collect()
}
pub fn descriptors(paths: &[PathBuf], device: &str) -> Vec<Value> {
    let Ok(root) = storage_root() else {
        return Vec::new();
    };
    paths
        .iter()
        .filter_map(|path| {
            if !path.starts_with(&root) {
                return None;
            }
            let id = path.parent()?.file_name()?.to_str()?;
            Some(match manifest(&root, id, device) {
                Ok((_, m)) if m.complete => json!({"id":id,"name":m.name,"available":true}),
                _ => json!({"name":"Image attached from another device","available":false}),
            })
        })
        .collect()
}
pub fn preview(id: &str, device: &str) -> RemoteResult<Value> {
    let path = resolve(&[id.into()], device)?.remove(0);
    let source = image::open(path).map_err(|e| RemoteError::internal(e.to_string()))?;
    let thumbnail = source.thumbnail(1280, 1280).to_rgb8();
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 75)
        .encode_image(&thumbnail)
        .map_err(|e| RemoteError::internal(e.to_string()))?;
    Ok(json!({"id":id,"base64":BASE64.encode(bytes),"mime":"image/jpeg"}))
}
fn perform(root: &Path, device: &str, payload: Value) -> RemoteResult<Value> {
    let _guard = UPLOAD_LOCK
        .lock()
        .map_err(|_| RemoteError::internal("Upload unavailable"))?;
    let io = |e: std::io::Error| RemoteError::internal(e.to_string());
    let operation = payload["operation"].as_str().unwrap_or_default();
    if operation == "begin" {
        let size = payload["size"].as_u64().unwrap_or(0) as usize;
        if size == 0 || size > MAX_IMAGE {
            return Err(RemoteError::bad_request("Images must be at most 4 MiB"));
        }
        let id = Uuid::new_v4().to_string();
        let dir = folder(root, &id)?;
        fs::create_dir_all(&dir).map_err(io)?;
        let m = Manifest {
            device: device.into(),
            name: payload["name"]
                .as_str()
                .unwrap_or("Image")
                .chars()
                .take(150)
                .collect(),
            size,
            complete: false,
            file_name: String::new(),
        };
        fs::write(dir.join("manifest.json"), serde_json::to_vec(&m).unwrap()).map_err(io)?;
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(dir.join("image"))
            .map_err(io)?;
        return Ok(json!({"id":id,"received":0,"complete":false}));
    }
    let id = payload["id"].as_str().unwrap_or_default();
    let (dir, mut m) = manifest(root, id, device)?;
    let path = dir.join("image");
    let received = fs::metadata(&path).map_err(io)?.len() as usize;
    match operation {
        "status" => Ok(json!({"id":id,"received":received,"complete":m.complete})),
        "chunk" => {
            let offset = payload["offset"]
                .as_u64()
                .ok_or_else(|| RemoteError::bad_request("Missing chunk offset"))?
                as usize;
            let data = BASE64
                .decode(payload["base64"].as_str().unwrap_or_default())
                .map_err(|_| RemoteError::bad_request("Invalid image chunk"))?;
            if m.complete
                || offset != received
                || data.is_empty()
                || data.len() > 256 * 1024
                || received + data.len() > m.size
            {
                return Err(RemoteError::conflict(
                    "Image upload changed; refresh its upload status",
                ));
            }
            let mut file = fs::OpenOptions::new().append(true).open(path).map_err(io)?;
            file.write_all(&data).map_err(io)?;
            file.sync_all().map_err(io)?;
            Ok(json!({"id":id,"received":received+data.len(),"complete":false}))
        }
        "finalize" => {
            if received != m.size {
                return Err(RemoteError::conflict("Image upload is incomplete"));
            }
            let bytes = fs::read(path).map_err(io)?;
            let reader = image::ImageReader::new(std::io::Cursor::new(&bytes))
                .with_guessed_format()
                .map_err(io)?;
            let format = reader.format();
            if !matches!(
                format,
                Some(image::ImageFormat::Png | image::ImageFormat::Jpeg | image::ImageFormat::WebP)
            ) {
                return Err(RemoteError::bad_request("Choose a JPEG, PNG or WebP image"));
            }
            let (w, h) = reader
                .into_dimensions()
                .map_err(|_| RemoteError::bad_request("Image could not be read"))?;
            if w == 0 || h == 0 || u64::from(w) * u64::from(h) > 32_000_000 {
                return Err(RemoteError::bad_request("Image dimensions are too large"));
            }
            image::load_from_memory(&bytes)
                .map_err(|_| RemoteError::bad_request("Image data is damaged"))?;
            m.file_name = match format.unwrap() {
                image::ImageFormat::Jpeg => "image.jpg",
                image::ImageFormat::Png => "image.png",
                _ => "image.webp",
            }
            .into();
            fs::write(dir.join(&m.file_name), &bytes).map_err(io)?;
            m.complete = true;
            fs::write(dir.join("manifest.json"), serde_json::to_vec(&m).unwrap()).map_err(io)?;
            Ok(json!({"id":id,"received":received,"complete":true}))
        }
        _ => Err(RemoteError::bad_request("Unknown upload operation")),
    }
}
pub async fn upload(
    Extension(device): Extension<PairedDevice>,
    Json(payload): Json<Value>,
) -> Response {
    let result =
        tokio::task::spawn_blocking(move || perform(&storage_root()?, &device.id, payload)).await;
    match result {
        Ok(Ok(value)) => Json(value).into_response(),
        Ok(Err(e)) => (
            axum::http::StatusCode::from_u16(e.status).unwrap(),
            Json(json!({"error":e.message})),
        )
            .into_response(),
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":"Upload failed"})),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn upload_resumes_validates_decoded_content_and_is_device_scoped() {
        let root = std::env::temp_dir().join(format!("choro-upload-{}", Uuid::new_v4()));
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(3, 3)
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        let bytes = encoded.into_inner();
        let call = |device: &str, payload: Value| perform(&root, device, payload);
        let begin = call("phone", json!({"operation":"begin","size":bytes.len()})).unwrap();
        let id = begin["id"].as_str().unwrap();
        assert!(call("other", json!({"operation":"status","id":id})).is_err());
        assert!(resolve_at(&root, &[id.into()], "phone").is_err());
        let split = bytes.len() / 2;
        call(
            "phone",
            json!({"operation":"chunk","id":id,"offset":0,"base64":BASE64.encode(&bytes[..split])}),
        )
        .unwrap();
        assert_eq!(
            call("phone", json!({"operation":"status","id":id})).unwrap()["received"],
            split
        );
        assert!(call("phone", json!({"operation":"finalize","id":id})).is_err());
        assert!(call(
            "phone",
            json!({"operation":"chunk","id":id,"offset":0,"base64":BASE64.encode(&bytes[split..])})
        )
        .is_err());
        call("phone",json!({"operation":"chunk","id":id,"offset":split,"base64":BASE64.encode(&bytes[split..])})).unwrap();
        call("phone", json!({"operation":"finalize","id":id})).unwrap();
        assert_eq!(
            fs::read(resolve_at(&root, &[id.into()], "phone").unwrap().remove(0)).unwrap(),
            bytes
        );
        assert!(resolve_at(&root, &[id.into()], "other").is_err());
        assert!(call("phone", json!({"operation":"begin","size":MAX_IMAGE+1})).is_err());
        let bad = call("phone", json!({"operation":"begin","size":4})).unwrap();
        call(
            "phone",
            json!({"operation":"chunk","id":bad["id"],"offset":0,"base64":BASE64.encode(b"fake")}),
        )
        .unwrap();
        assert!(call("phone", json!({"operation":"finalize","id":bad["id"]})).is_err());
    }
}
