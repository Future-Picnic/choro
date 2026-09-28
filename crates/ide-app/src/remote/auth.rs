use std::collections::HashMap;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

const PAIRING_TTL_SECS: u64 = 5 * 60;
const MAX_PAIRING_ATTEMPTS: u8 = 10;
const MAX_PAIRED_DEVICES: usize = 3;
const TOKEN_PREFIX: &str = "choro_device_";
const ADMISSION_TOKEN_PREFIX: &str = "choro_admit_";
const DEVICE_TTL_SECS: u64 = 90 * 24 * 60 * 60;
const KEYCHAIN_SERVICE: &str = "com.ritmus.choro.remote.transport";
// PocketComet polls every 30 seconds while idle; allow a delayed request.
const POCKETCOMET_PRESENCE_TTL: Duration = Duration::from_secs(65);

#[derive(Clone)]
pub struct RemoteAuth {
    inner: Arc<Mutex<AuthState>>,
    storage_path: Arc<PathBuf>,
    use_keychain: bool,
}

#[derive(Default)]
struct AuthState {
    devices: Vec<StoredDevice>,
    pairing: Option<PairingWindow>,
    pending_pairing: Option<PendingPairing>,
    storage_error: Option<String>,
    authenticated_at: HashMap<String, Instant>,
}

#[derive(Clone)]
struct PairingWindow {
    code: String,
    expires_at: u64,
    failed_attempts: u8,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredDevice {
    id: String,
    name: String,
    token_hash: String,
    #[serde(default)]
    transport_key: Option<String>,
    #[serde(default)]
    admission_token_hash: String,
    #[serde(default)]
    permission: DevicePermission,
    paired_at: u64,
    last_seen_at: u64,
    #[serde(default)]
    expires_at: u64,
    #[serde(default)]
    pairing_transaction_id: Option<String>,
}

// No raw bearer or admission credentials are written to disk. A prepared phone
// must durably save its credentials before committing this record.
#[derive(Clone, Serialize, Deserialize)]
struct PendingPairing {
    transaction_id: String,
    device: StoredDevice,
    replaces: Option<String>,
    expires_at: u64,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DevicePermission {
    ViewOnly,
    #[default]
    Control,
    FullAccess,
}

#[derive(Default, Serialize, Deserialize)]
struct StoredAuthFile {
    #[serde(default)]
    devices: Vec<StoredDevice>,
    #[serde(default)]
    pending_pairing: Option<PendingPairing>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct PairedDevice {
    pub id: String,
    pub name: String,
    pub paired_at: u64,
    pub last_seen_at: u64,
    pub expires_at: u64,
    pub permission: DevicePermission,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairingSnapshot {
    pub active_code: Option<String>,
    pub expires_at: Option<u64>,
    pub devices: Vec<PairedDevice>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct PairingPublicStatus {
    pub pairing_active: bool,
    pub paired_device_count: usize,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct PairingResult {
    pub token: String,
    pub admission_token: String,
    pub device: PairedDevice,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct PairingPreparation {
    pub transaction_id: String,
    pub token: String,
    pub admission_token: String,
    pub device: PairedDevice,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PairingError {
    NotActive,
    Expired,
    InvalidCode,
    TooManyAttempts,
    DeviceLimit,
    Storage(String),
}

impl RemoteAuth {
    pub fn load_default() -> Self {
        let storage_path = ide_core::local_store::LocalStore::open_default()
            .map(|store| {
                store
                    .app_data_dir()
                    .join("remote")
                    .join("paired-devices.json")
            })
            .unwrap_or_else(|_| PathBuf::from("paired-devices.json"));
        Self::load_with_keychain(storage_path, cfg!(target_os = "macos"))
    }

    #[cfg(test)]
    pub(super) fn load(storage_path: PathBuf) -> Self {
        Self::load_with_keychain(storage_path, false)
    }

    fn load_with_keychain(storage_path: PathBuf, use_keychain: bool) -> Self {
        let (stored, storage_error) = match fs::read(&storage_path) {
            Ok(bytes) => match serde_json::from_slice::<StoredAuthFile>(&bytes) {
                Ok(stored) => (stored, None),
                Err(error) => (
                    StoredAuthFile::default(),
                    Some(format!(
                        "Device storage is damaged; restore it before pairing: {error}"
                    )),
                ),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                (StoredAuthFile::default(), None)
            }
            Err(error) => (
                StoredAuthFile::default(),
                Some(format!(
                    "Device storage is unavailable; restore access before pairing: {error}"
                )),
            ),
        };
        let mut devices = stored.devices;
        for device in &mut devices {
            if device.expires_at == 0 {
                device.expires_at = device.paired_at.saturating_add(DEVICE_TTL_SECS);
            }
        }
        let mut migrated_secret = false;
        if use_keychain {
            for device in &mut devices {
                if let Some(secret) = device.transport_key.clone() {
                    if keychain_set(&device.id, &secret).is_ok() {
                        device.transport_key = None;
                        migrated_secret = true;
                    }
                }
            }
        }
        let auth = Self {
            inner: Arc::new(Mutex::new(AuthState {
                devices,
                pairing: None,
                pending_pairing: stored
                    .pending_pairing
                    .filter(|pending| pending.expires_at > unix_now()),
                storage_error,
                authenticated_at: HashMap::new(),
            })),
            storage_path: Arc::new(storage_path),
            use_keychain,
        };
        if migrated_secret {
            let state = auth.inner.lock();
            if let Err(error) = auth.persist_locked(&state) {
                eprintln!("failed to finish Choro remote Keychain migration: {error:?}");
            }
        }
        auth
    }

    pub fn start_pairing(&self) -> PairingSnapshot {
        let now = unix_now();
        let raw = Uuid::new_v4().simple().to_string().to_ascii_uppercase();
        let code = raw
            .as_bytes()
            .chunks(4)
            .map(|chunk| std::str::from_utf8(chunk).unwrap_or_default())
            .collect::<Vec<_>>()
            .join("-");
        let mut state = self.inner.lock();
        // Starting a new window explicitly cancels a prepared transaction, but
        // never changes a currently authorized device.
        let previous_pending = state.pending_pairing.take();
        if previous_pending.is_some() && self.persist_locked(&state).is_err() {
            state.pending_pairing = previous_pending;
            return snapshot_locked(&mut state, now);
        }
        state.pairing = Some(PairingWindow {
            code,
            expires_at: now + PAIRING_TTL_SECS,
            failed_attempts: 0,
        });
        snapshot_locked(&mut state, now)
    }

    pub fn cancel_pairing(&self) -> PairingSnapshot {
        let now = unix_now();
        let mut state = self.inner.lock();
        let previous_pending = state.pending_pairing.take();
        if previous_pending.is_some() && self.persist_locked(&state).is_err() {
            state.pending_pairing = previous_pending;
            return snapshot_locked(&mut state, now);
        }
        state.pairing = None;
        snapshot_locked(&mut state, now)
    }

    pub fn snapshot(&self) -> PairingSnapshot {
        let now = unix_now();
        snapshot_locked(&mut self.inner.lock(), now)
    }

    pub fn public_status(&self) -> PairingPublicStatus {
        let snapshot = self.snapshot();
        PairingPublicStatus {
            pairing_active: snapshot.active_code.is_some(),
            paired_device_count: snapshot.devices.len(),
        }
    }

    pub fn pair(&self, code: &str, device_name: &str) -> Result<PairingResult, PairingError> {
        self.pair_with_transport_key(code, device_name, None)
    }

    pub fn pair_with_transport_key(
        &self,
        code: &str,
        device_name: &str,
        transport_key: Option<String>,
    ) -> Result<PairingResult, PairingError> {
        let now = unix_now();
        let normalized = normalize_pairing_code(code);
        let mut state = self.inner.lock();
        if state
            .pending_pairing
            .as_ref()
            .is_some_and(|pending| pending.expires_at > now)
        {
            return Err(PairingError::NotActive);
        }
        let Some(pairing) = state.pairing.as_mut() else {
            return Err(PairingError::NotActive);
        };
        if pairing.expires_at <= now {
            state.pairing = None;
            return Err(PairingError::Expired);
        }
        if !constant_time_eq(
            normalize_pairing_code(&pairing.code).as_bytes(),
            normalized.as_bytes(),
        ) {
            pairing.failed_attempts = pairing.failed_attempts.saturating_add(1);
            if pairing.failed_attempts >= MAX_PAIRING_ATTEMPTS {
                state.pairing = None;
                return Err(PairingError::TooManyAttempts);
            }
            return Err(PairingError::InvalidCode);
        }
        if state.devices.len() >= MAX_PAIRED_DEVICES {
            return Err(PairingError::DeviceLimit);
        }

        let clean_name = device_name.trim();
        let name = if clean_name.is_empty() {
            "iPhone".to_string()
        } else {
            clean_name.chars().take(80).collect()
        };
        let token = format!(
            "{TOKEN_PREFIX}{}{}",
            Uuid::new_v4().simple(),
            Uuid::new_v4().simple()
        );
        let admission_token = format!(
            "{ADMISSION_TOKEN_PREFIX}{}{}",
            Uuid::new_v4().simple(),
            Uuid::new_v4().simple()
        );
        let device_id = Uuid::new_v4().to_string();
        let stored_transport_key = if self.use_keychain {
            if let Some(secret) = transport_key.as_deref() {
                keychain_set(&device_id, secret)
                    .map_err(|error| PairingError::Storage(format!("Keychain: {error}")))?;
            }
            None
        } else {
            transport_key
        };
        let stored = StoredDevice {
            id: device_id.clone(),
            name,
            token_hash: token_hash(&token),
            transport_key: stored_transport_key,
            admission_token_hash: token_hash(&admission_token),
            permission: DevicePermission::Control,
            paired_at: now,
            last_seen_at: now,
            expires_at: now + DEVICE_TTL_SECS,
            pairing_transaction_id: None,
        };
        let previous_pairing = state.pairing.clone();
        state.devices.push(stored.clone());
        state.pairing = None;
        if let Err(error) = self.persist_locked(&state) {
            state.devices.pop();
            state.pairing = previous_pairing;
            if self.use_keychain {
                let _ = keychain_delete(&device_id);
            }
            return Err(error);
        }
        Ok(PairingResult {
            token,
            admission_token,
            device: public_device(&stored),
        })
    }

    /// Reserve one pairing without invalidating an existing phone. The returned
    /// secrets live only in memory and on the phone; the journal contains hashes.
    pub fn prepare_pairing(
        &self,
        code: &str,
        device_name: &str,
        previous_token: Option<&str>,
    ) -> Result<PairingPreparation, PairingError> {
        let now = unix_now();
        let mut state = self.inner.lock();
        if state
            .pending_pairing
            .as_ref()
            .is_some_and(|pending| pending.expires_at > now)
        {
            return Err(PairingError::NotActive);
        }
        let Some(pairing) = state.pairing.as_mut() else {
            return Err(PairingError::NotActive);
        };
        if pairing.expires_at <= now {
            state.pairing = None;
            return Err(PairingError::Expired);
        }
        if code.len() > 256
            || !constant_time_eq(
                normalize_pairing_code(&pairing.code).as_bytes(),
                normalize_pairing_code(code).as_bytes(),
            )
        {
            pairing.failed_attempts = pairing.failed_attempts.saturating_add(1);
            if pairing.failed_attempts >= MAX_PAIRING_ATTEMPTS {
                state.pairing = None;
                return Err(PairingError::TooManyAttempts);
            }
            return Err(PairingError::InvalidCode);
        }
        // An invalid supplied credential cannot be used to evict any device.
        let replacement = previous_token
            .filter(|token| token.starts_with(TOKEN_PREFIX) && token.len() <= 256)
            .and_then(|token| {
                let hash = token_hash(token);
                state.devices.iter().find(|device| {
                    device.expires_at > now
                        && constant_time_eq(device.token_hash.as_bytes(), hash.as_bytes())
                })
            });
        if replacement.is_none() && state.devices.len() >= MAX_PAIRED_DEVICES {
            return Err(PairingError::DeviceLimit);
        }
        let transaction_id = Uuid::new_v4().to_string();
        let token = format!(
            "{TOKEN_PREFIX}{}{}",
            Uuid::new_v4().simple(),
            Uuid::new_v4().simple()
        );
        let admission_token = format!(
            "{ADMISSION_TOKEN_PREFIX}{}{}",
            Uuid::new_v4().simple(),
            Uuid::new_v4().simple()
        );
        let name = device_name.trim();
        let device = StoredDevice {
            id: Uuid::new_v4().to_string(),
            name: if name.is_empty() {
                "iPhone".into()
            } else {
                name.chars().take(80).collect()
            },
            token_hash: token_hash(&token),
            transport_key: None,
            admission_token_hash: token_hash(&admission_token),
            permission: replacement
                .map(|device| device.permission)
                .unwrap_or_default(),
            paired_at: now,
            last_seen_at: now,
            expires_at: now + DEVICE_TTL_SECS,
            pairing_transaction_id: Some(transaction_id.clone()),
        };
        let pending = PendingPairing {
            transaction_id: transaction_id.clone(),
            device: device.clone(),
            replaces: replacement.map(|device| device.id.clone()),
            expires_at: now + PAIRING_TTL_SECS,
        };
        let previous_pending = state.pending_pairing.replace(pending);
        if let Err(error) = self.persist_locked(&state) {
            state.pending_pairing = previous_pending;
            return Err(error);
        }
        Ok(PairingPreparation {
            transaction_id,
            token,
            admission_token,
            device: public_device(&device),
        })
    }

    /// Idempotent across reconnection and restart. A revoked credential cannot
    /// resurrect itself by repeating a previously committed transaction.
    pub fn commit_pairing(
        &self,
        transaction_id: &str,
        token: &str,
    ) -> Result<PairedDevice, PairingError> {
        if transaction_id.len() > 128 || !token.starts_with(TOKEN_PREFIX) || token.len() > 256 {
            return Err(PairingError::InvalidCode);
        }
        let now = unix_now();
        let hash = token_hash(token);
        let mut state = self.inner.lock();
        if let Some(device) = state.devices.iter().find(|device| {
            device.pairing_transaction_id.as_deref() == Some(transaction_id)
                && device.expires_at > now
                && constant_time_eq(device.token_hash.as_bytes(), hash.as_bytes())
        }) {
            return Ok(public_device(device));
        }
        let pending = state
            .pending_pairing
            .as_ref()
            .ok_or(PairingError::NotActive)?;
        if pending.expires_at <= now {
            return Err(PairingError::Expired);
        }
        if pending.transaction_id != transaction_id
            || !constant_time_eq(pending.device.token_hash.as_bytes(), hash.as_bytes())
        {
            return Err(PairingError::InvalidCode);
        }
        let mut device = pending.device.clone();
        let replaces = pending.replaces.clone();
        if let Some(id) = replaces.as_deref() {
            // Honor revocation and permission changes made while pairing.
            let old = state
                .devices
                .iter()
                .find(|old| old.id == id && old.expires_at > now)
                .ok_or(PairingError::NotActive)?;
            device.permission = old.permission;
        } else if state.devices.len() >= MAX_PAIRED_DEVICES {
            return Err(PairingError::DeviceLimit);
        }
        let previous_devices = state.devices.clone();
        let previous_pending = state.pending_pairing.take();
        let previous_pairing = state.pairing.take();
        state
            .devices
            .retain(|old| Some(old.id.as_str()) != replaces.as_deref());
        state.devices.push(device.clone());
        if let Err(error) = self.persist_locked(&state) {
            state.devices = previous_devices;
            state.pending_pairing = previous_pending;
            state.pairing = previous_pairing;
            return Err(error);
        }
        if let Some(id) = replaces {
            state.authenticated_at.remove(&id);
            // Preserve the legacy Keychain entry; it is no longer authorized.
        }
        Ok(public_device(&device))
    }

    pub fn authorize(&self, token: &str) -> bool {
        self.authorize_device(token, None).is_some()
    }

    pub fn authorize_device(
        &self,
        token: &str,
        expected_device_id: Option<&str>,
    ) -> Option<PairedDevice> {
        if !token.starts_with(TOKEN_PREFIX) || token.len() > 256 {
            return None;
        }
        let hash = token_hash(token);
        let now = unix_now();
        let mut state = self.inner.lock();
        for device in &mut state.devices {
            if expected_device_id.is_none_or(|expected| expected == device.id)
                && device.expires_at > now
                && constant_time_eq(device.token_hash.as_bytes(), hash.as_bytes())
            {
                device.last_seen_at = now;
                let device = public_device(device);
                state
                    .authenticated_at
                    .insert(device.id.clone(), Instant::now());
                return Some(device);
            }
        }
        None
    }

    /// Live activity only: saved pairing timestamps never establish presence.
    pub fn pocketcomet_connected(&self) -> bool {
        let now = unix_now();
        let state = self.inner.lock();
        state.devices.iter().any(|device| {
            device.name.starts_with("PocketComet on ")
                && device.expires_at > now
                && state
                    .authenticated_at
                    .get(&device.id)
                    .is_some_and(|seen| seen.elapsed() < POCKETCOMET_PRESENCE_TTL)
        })
    }

    pub fn is_device_active(&self, device_id: &str) -> bool {
        let now = unix_now();
        self.inner
            .lock()
            .devices
            .iter()
            .any(|device| device.id == device_id && device.expires_at > now)
    }

    pub fn admission_token_hashes(&self) -> Vec<String> {
        let now = unix_now();
        let state = self.inner.lock();
        state
            .devices
            .iter()
            .filter(|device| device.expires_at > now && !device.admission_token_hash.is_empty())
            .map(|device| device.admission_token_hash.clone())
            .collect()
    }

    pub fn pending_admission_token_hashes(&self) -> Vec<String> {
        let now = unix_now();
        let state = self.inner.lock();
        // Allows a phone that saved its prepared credentials to recover commit
        // on a new TLS stream, including after a Mac process restart.
        state
            .pending_pairing
            .as_ref()
            .filter(|pending| pending.expires_at > now)
            .map(|pending| vec![pending.device.admission_token_hash.clone()])
            .unwrap_or_default()
    }

    pub fn device_permission(&self, device_id: &str) -> Option<DevicePermission> {
        let now = unix_now();
        self.inner
            .lock()
            .devices
            .iter()
            .find(|device| device.id == device_id && device.expires_at > now)
            .map(|device| device.permission)
    }

    pub fn set_device_permission(
        &self,
        device_id: &str,
        permission: DevicePermission,
    ) -> Result<bool, PairingError> {
        let mut state = self.inner.lock();
        let previous_devices = state.devices.clone();
        let Some(device) = state
            .devices
            .iter_mut()
            .find(|device| device.id == device_id)
        else {
            return Ok(false);
        };
        device.permission = permission;
        if let Err(error) = self.persist_locked(&state) {
            state.devices = previous_devices;
            return Err(error);
        }
        Ok(true)
    }

    pub fn transport_key(&self, device_id: &str) -> Option<String> {
        let stored = self
            .inner
            .lock()
            .devices
            .iter()
            .find(|device| device.id == device_id)
            .and_then(|device| device.transport_key.clone());
        if stored.is_some() || !self.use_keychain {
            return stored;
        }
        keychain_get(device_id).ok()
    }

    pub fn revoke(&self, device_id: &str) -> Result<bool, PairingError> {
        let mut state = self.inner.lock();
        let previous_devices = state.devices.clone();
        let before = state.devices.len();
        state.devices.retain(|device| device.id != device_id);
        let removed = state.devices.len() != before;
        if removed {
            if let Err(error) = self.persist_locked(&state) {
                state.devices = previous_devices;
                return Err(error);
            }
            state.authenticated_at.remove(device_id);
            if self.use_keychain {
                let _ = keychain_delete(device_id);
            }
        }
        Ok(removed)
    }

    fn persist_locked(&self, state: &AuthState) -> Result<(), PairingError> {
        if let Some(error) = &state.storage_error {
            return Err(PairingError::Storage(error.clone()));
        }
        let file = StoredAuthFile {
            devices: state.devices.clone(),
            pending_pairing: state.pending_pairing.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&file)
            .map_err(|error| PairingError::Storage(error.to_string()))?;
        write_private_atomic(&self.storage_path, &bytes)
            .map_err(|error| PairingError::Storage(error.to_string()))
    }
}

fn snapshot_locked(state: &mut AuthState, now: u64) -> PairingSnapshot {
    if state
        .pairing
        .as_ref()
        .is_some_and(|pairing| pairing.expires_at <= now)
    {
        state.pairing = None;
    }
    PairingSnapshot {
        active_code: state.pairing.as_ref().map(|pairing| pairing.code.clone()),
        expires_at: state.pairing.as_ref().map(|pairing| pairing.expires_at),
        devices: state.devices.iter().map(public_device).collect(),
    }
}

fn public_device(device: &StoredDevice) -> PairedDevice {
    PairedDevice {
        id: device.id.clone(),
        name: device.name.clone(),
        paired_at: device.paired_at,
        last_seen_at: device.last_seen_at,
        expires_at: device.expires_at,
        permission: device.permission,
    }
}

fn normalize_pairing_code(code: &str) -> String {
    code.chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_uppercase)
        .collect()
}

fn token_hash(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let max_len = left.len().max(right.len());
    let mut difference = left.len() ^ right.len();
    for index in 0..max_len {
        let left_byte = left.get(index).copied().unwrap_or(0);
        let right_byte = right.get(index).copied().unwrap_or(0);
        difference |= usize::from(left_byte ^ right_byte);
    }
    difference == 0
}

#[cfg(target_os = "macos")]
fn keychain_set(account: &str, secret: &str) -> Result<(), String> {
    security_framework::passwords::set_generic_password(
        &transport_keychain_service(),
        account,
        secret.as_bytes(),
    )
    .map_err(|error| error.to_string())
}

#[cfg(not(target_os = "macos"))]
fn keychain_set(_account: &str, _secret: &str) -> Result<(), String> {
    Err("Keychain is unavailable".into())
}

#[cfg(target_os = "macos")]
fn keychain_get(account: &str) -> Result<String, String> {
    let bytes =
        security_framework::passwords::get_generic_password(&transport_keychain_service(), account)
            .map_err(|error| error.to_string())?;
    String::from_utf8(bytes).map_err(|error| error.to_string())
}

#[cfg(not(target_os = "macos"))]
fn keychain_get(_account: &str) -> Result<String, String> {
    Err("Keychain is unavailable".into())
}

#[cfg(target_os = "macos")]
fn keychain_delete(account: &str) -> Result<(), String> {
    security_framework::passwords::delete_generic_password(&transport_keychain_service(), account)
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
fn transport_keychain_service() -> String {
    std::env::var("CHORO_TRANSPORT_KEYCHAIN_SERVICE").unwrap_or_else(|_| KEYCHAIN_SERVICE.into())
}

#[cfg(not(target_os = "macos"))]
fn keychain_delete(_account: &str) -> Result<(), String> {
    Ok(())
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

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn damaged_device_storage_is_preserved_until_explicit_recovery() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("devices.json");
        let damaged = b"{\"devices\":[\"incomplete existing device data";
        fs::write(&path, damaged).unwrap();
        let auth = RemoteAuth::load(path.clone());
        let code = auth.start_pairing().active_code.unwrap();
        assert!(matches!(
            auth.prepare_pairing(&code, "Phone", None),
            Err(PairingError::Storage(_))
        ));
        assert!(matches!(
            auth.pair(&code, "Legacy phone"),
            Err(PairingError::Storage(_))
        ));
        assert_eq!(fs::read(path).unwrap(), damaged);
    }

    #[test]
    fn transactional_migration_preserves_three_devices_until_durable_commit() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("devices.json");
        let auth = RemoteAuth::load(path.clone());
        let code = auth.start_pairing().active_code.unwrap();
        let original = auth.pair(&code, "Original phone").unwrap();
        for name in ["Phone 2", "Phone 3"] {
            let code = auth.start_pairing().active_code.unwrap();
            auth.pair(&code, name).unwrap();
        }
        auth.set_device_permission(&original.device.id, DevicePermission::FullAccess)
            .unwrap();
        let code = auth.start_pairing().active_code.unwrap();
        assert_eq!(
            auth.prepare_pairing(&code, "Fourth phone", None),
            Err(PairingError::DeviceLimit)
        );
        let prepared = auth
            .prepare_pairing(&code, "Updated phone", Some(&original.token))
            .unwrap();
        assert_eq!(prepared.device.permission, DevicePermission::FullAccess);
        assert!(auth.authorize(&original.token));
        assert!(!auth.authorize(&prepared.token));
        assert_eq!(auth.snapshot().devices.len(), 3);
        assert_eq!(auth.admission_token_hashes().len(), 3);
        assert_eq!(auth.pending_admission_token_hashes().len(), 1);
        let journal = fs::read_to_string(&path).unwrap();
        assert!(!journal.contains(&prepared.token));
        assert!(!journal.contains(&prepared.admission_token));
        assert_eq!(
            auth.prepare_pairing(&code, "Concurrent phone", None),
            Err(PairingError::NotActive)
        );
        assert_eq!(
            auth.pair(&code, "Legacy concurrent phone"),
            Err(PairingError::NotActive)
        );

        // A Mac restart and a lost commit response cannot invalidate the staged
        // credential or create a fourth record.
        let restarted = RemoteAuth::load(path.clone());
        restarted
            .set_device_permission(&original.device.id, DevicePermission::ViewOnly)
            .unwrap();
        let committed = restarted
            .commit_pairing(&prepared.transaction_id, &prepared.token)
            .unwrap();
        assert_eq!(committed.permission, DevicePermission::ViewOnly);
        assert_eq!(restarted.snapshot().devices.len(), 3);
        assert!(!restarted.authorize(&original.token));
        assert!(restarted.authorize(&prepared.token));
        assert!(restarted.pending_admission_token_hashes().is_empty());
        let reloaded = RemoteAuth::load(path);
        assert_eq!(
            reloaded
                .commit_pairing(&prepared.transaction_id, &prepared.token)
                .unwrap()
                .id,
            prepared.device.id
        );
        reloaded.revoke(&prepared.device.id).unwrap();
        assert!(reloaded
            .commit_pairing(&prepared.transaction_id, &prepared.token)
            .is_err());
    }

    #[test]
    fn transaction_expiry_cancellation_and_revocation_preserve_old_credentials() {
        let directory = tempfile::tempdir().unwrap();
        let auth = RemoteAuth::load(directory.path().join("devices.json"));
        let code = auth.start_pairing().active_code.unwrap();
        let original = auth.pair(&code, "Original phone").unwrap();
        let code = auth.start_pairing().active_code.unwrap();
        let prepared = auth
            .prepare_pairing(&code, "Updated phone", Some(&original.token))
            .unwrap();
        assert_eq!(
            auth.commit_pairing(&prepared.transaction_id, "choro_device_wrong"),
            Err(PairingError::InvalidCode)
        );
        auth.inner
            .lock()
            .pending_pairing
            .as_mut()
            .unwrap()
            .expires_at = unix_now();
        assert_eq!(
            auth.commit_pairing(&prepared.transaction_id, &prepared.token),
            Err(PairingError::Expired)
        );
        assert!(auth.authorize(&original.token));
        assert!(auth.pending_admission_token_hashes().is_empty());

        let code = auth.start_pairing().active_code.unwrap();
        let prepared = auth
            .prepare_pairing(&code, "Updated phone", Some(&original.token))
            .unwrap();
        auth.cancel_pairing();
        assert!(auth
            .commit_pairing(&prepared.transaction_id, &prepared.token)
            .is_err());
        assert!(auth.authorize(&original.token));

        let code = auth.start_pairing().active_code.unwrap();
        let prepared = auth
            .prepare_pairing(&code, "Updated phone", Some(&original.token))
            .unwrap();
        auth.revoke(&original.device.id).unwrap();
        assert_eq!(
            auth.commit_pairing(&prepared.transaction_id, &prepared.token),
            Err(PairingError::NotActive)
        );
        assert!(!auth.authorize(&prepared.token));
    }

    #[test]
    fn commit_storage_failure_rolls_back_both_credentials_and_journal() {
        let directory = tempfile::tempdir().unwrap();
        let storage = directory.path().join("remote");
        let auth = RemoteAuth::load(storage.join("devices.json"));
        let code = auth.start_pairing().active_code.unwrap();
        let old = auth.pair(&code, "Old phone").unwrap();
        let code = auth.start_pairing().active_code.unwrap();
        let prepared = auth
            .prepare_pairing(&code, "Updated phone", Some(&old.token))
            .unwrap();
        fs::rename(&storage, directory.path().join("remote-preserved")).unwrap();
        fs::write(&storage, b"blocked").unwrap();
        assert!(matches!(
            auth.commit_pairing(&prepared.transaction_id, &prepared.token),
            Err(PairingError::Storage(_))
        ));
        assert!(auth.authorize(&old.token));
        assert!(!auth.authorize(&prepared.token));
        assert_eq!(auth.pending_admission_token_hashes().len(), 1);
    }

    #[test]
    fn secure_pairing_enforces_attempt_limit_and_one_time_code() {
        let directory = tempfile::tempdir().unwrap();
        let auth = RemoteAuth::load(directory.path().join("devices.json"));
        auth.start_pairing();
        for _ in 0..(MAX_PAIRING_ATTEMPTS - 1) {
            assert_eq!(
                auth.prepare_pairing("WRONG", "Phone", None),
                Err(PairingError::InvalidCode)
            );
        }
        assert_eq!(
            auth.prepare_pairing("WRONG", "Phone", None),
            Err(PairingError::TooManyAttempts)
        );
        let code = auth.start_pairing().active_code.unwrap();
        let prepared = auth.prepare_pairing(&code, "Phone", None).unwrap();
        auth.commit_pairing(&prepared.transaction_id, &prepared.token)
            .unwrap();
        assert_eq!(
            auth.prepare_pairing(&code, "Other phone", None),
            Err(PairingError::NotActive)
        );
    }

    #[test]
    fn pocketcomet_presence_requires_live_authentication_and_clears_on_revoke() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("devices.json");
        let auth = RemoteAuth::load(path.clone());
        let code = auth.start_pairing().active_code.unwrap();
        let phone = auth.pair(&code, "iPhone").unwrap();
        assert!(auth.authorize(&phone.token));
        assert!(!auth.pocketcomet_connected());

        let code = auth.start_pairing().active_code.unwrap();
        let comet = auth.pair(&code, "PocketComet on this Mac").unwrap();
        assert!(!auth.pocketcomet_connected());
        assert!(!auth.authorize("choro_device_invalid"));
        assert!(!auth.pocketcomet_connected());
        assert!(auth.authorize(&comet.token));
        assert!(auth.pocketcomet_connected());
        assert!(!RemoteAuth::load(path).pocketcomet_connected());
        assert!(auth.revoke(&comet.device.id).unwrap());
        assert!(!auth.pocketcomet_connected());
    }

    #[test]
    fn pocketcomet_presence_tolerates_idle_polling_but_expires() {
        let directory = tempfile::tempdir().unwrap();
        let auth = RemoteAuth::load(directory.path().join("devices.json"));
        let code = auth.start_pairing().active_code.unwrap();
        let comet = auth.pair(&code, "PocketComet on this Mac").unwrap();
        assert!(auth.authorize(&comet.token));
        auth.inner.lock().authenticated_at.insert(
            comet.device.id.clone(),
            Instant::now() - Duration::from_secs(30),
        );
        assert!(auth.pocketcomet_connected());
        auth.inner.lock().authenticated_at.insert(
            comet.device.id.clone(),
            Instant::now() - POCKETCOMET_PRESENCE_TTL,
        );
        assert!(!auth.pocketcomet_connected());
        assert!(auth.authorize(&comet.token));
        assert!(auth.pocketcomet_connected());
        auth.inner.lock().devices[0].expires_at = unix_now();
        assert!(!auth.pocketcomet_connected());
    }

    #[test]
    fn pairing_is_one_time_and_tokens_survive_reload() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("devices.json");
        let auth = RemoteAuth::load(path.clone());
        let code = auth.start_pairing().active_code.unwrap();
        let paired = auth.pair(&code, "Liran's iPhone").unwrap();
        assert!(paired.admission_token.starts_with(ADMISSION_TOKEN_PREFIX));
        assert_eq!(auth.admission_token_hashes().len(), 1);
        assert!(auth.authorize(&paired.token));
        assert_eq!(auth.pair(&code, "Other"), Err(PairingError::NotActive));

        let reloaded = RemoteAuth::load(path);
        assert!(reloaded.authorize(&paired.token));
        assert_eq!(reloaded.snapshot().devices[0].name, "Liran's iPhone");
    }

    #[test]
    fn invalid_attempts_close_the_pairing_window() {
        let directory = tempfile::tempdir().unwrap();
        let auth = RemoteAuth::load(directory.path().join("devices.json"));
        auth.start_pairing();
        for _ in 0..(MAX_PAIRING_ATTEMPTS - 1) {
            assert_eq!(auth.pair("WRONG", "iPhone"), Err(PairingError::InvalidCode));
        }
        assert_eq!(
            auth.pair("WRONG", "iPhone"),
            Err(PairingError::TooManyAttempts)
        );
        assert!(!auth.public_status().pairing_active);
    }

    #[test]
    fn revocation_immediately_invalidates_a_device() {
        let directory = tempfile::tempdir().unwrap();
        let auth = RemoteAuth::load(directory.path().join("devices.json"));
        let code = auth.start_pairing().active_code.unwrap();
        let paired = auth.pair(&code, "iPhone").unwrap();
        assert!(auth.revoke(&paired.device.id).unwrap());
        assert!(!auth.authorize(&paired.token));
        assert!(auth.admission_token_hashes().is_empty());
    }

    #[test]
    fn permissions_are_desktop_controlled_and_persisted() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("devices.json");
        let auth = RemoteAuth::load(path.clone());
        let code = auth.start_pairing().active_code.unwrap();
        let paired = auth.pair(&code, "iPhone").unwrap();
        assert_eq!(paired.device.permission, DevicePermission::Control);
        assert!(auth
            .set_device_permission(&paired.device.id, DevicePermission::FullAccess)
            .unwrap());
        let reloaded = RemoteAuth::load(path);
        assert_eq!(
            reloaded.device_permission(&paired.device.id),
            Some(DevicePermission::FullAccess)
        );
    }

    #[test]
    fn pairing_rejects_devices_past_the_storage_limit() {
        let directory = tempfile::tempdir().unwrap();
        let auth = RemoteAuth::load(directory.path().join("devices.json"));
        for index in 0..MAX_PAIRED_DEVICES {
            let code = auth.start_pairing().active_code.unwrap();
            auth.pair(&code, &format!("iPhone {index}")).unwrap();
        }
        let code = auth.start_pairing().active_code.unwrap();
        assert_eq!(
            auth.pair(&code, "One too many"),
            Err(PairingError::DeviceLimit)
        );
        assert!(auth.public_status().pairing_active);
    }

    #[test]
    fn expired_credentials_are_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let auth = RemoteAuth::load(directory.path().join("devices.json"));
        let code = auth.start_pairing().active_code.unwrap();
        let paired = auth.pair(&code, "iPhone").unwrap();
        auth.inner.lock().devices[0].expires_at = unix_now();
        assert!(!auth.authorize(&paired.token));
        assert!(auth.admission_token_hashes().is_empty());
    }

    #[test]
    fn failed_pairing_persistence_rolls_back_memory_state() {
        let directory = tempfile::tempdir().unwrap();
        let blocked_parent = directory.path().join("not-a-directory");
        fs::write(&blocked_parent, b"blocked").unwrap();
        let auth = RemoteAuth::load(blocked_parent.join("devices.json"));
        let code = auth.start_pairing().active_code.unwrap();

        assert!(matches!(
            auth.pair(&code, "iPhone"),
            Err(PairingError::Storage(_))
        ));
        let snapshot = auth.snapshot();
        assert_eq!(snapshot.active_code.as_deref(), Some(code.as_str()));
        assert!(snapshot.devices.is_empty());
    }

    #[test]
    fn failed_revocation_persistence_keeps_device_authorized() {
        let directory = tempfile::tempdir().unwrap();
        let storage_directory = directory.path().join("remote");
        let storage_path = storage_directory.join("devices.json");
        let auth = RemoteAuth::load(storage_path);
        let code = auth.start_pairing().active_code.unwrap();
        let paired = auth.pair(&code, "iPhone").unwrap();

        let backup = directory.path().join("remote-backup");
        fs::rename(&storage_directory, &backup).unwrap();
        fs::write(&storage_directory, b"blocked").unwrap();

        assert!(matches!(
            auth.revoke(&paired.device.id),
            Err(PairingError::Storage(_))
        ));
        assert!(auth.authorize(&paired.token));
    }
}
