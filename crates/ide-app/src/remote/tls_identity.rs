//! Per-installation TLS authority. Only public identity metadata is stored in
//! Choro's data directory; the authority key is held in macOS Keychain. Leaf
//! keys are ephemeral process memory and never enter a pairing payload.
use std::{fs, io::Write, path::Path, sync::Arc};

use anyhow::{bail, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose,
};
use rustls::{
    client::danger::ServerCertVerifier,
    pki_types::{CertificateDer, PrivatePkcs8KeyDer, ServerName, UnixTime},
    RootCertStore, ServerConfig,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

pub const TLS_HOSTNAME: &str = "desktop.choro.invalid";
pub const TLS_ALPN: &[u8] = b"choro-remote/3";
const KEYCHAIN_SERVICE: &str = "com.futurepicnic.choro.remote.tls.v1";
const RENEW_BEFORE_DAYS: i64 = 30;

#[derive(Serialize, Deserialize)]
struct Metadata {
    version: u8,
    account: String,
    // None is an initialization reservation, never permission to replace a key.
    authority_sha256: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct AuthoritySecret {
    version: u8,
    authority_der: String,
    private_key_pkcs8: String,
    created_at: i64,
    expires_at: i64,
}

pub struct TlsIdentity {
    authority: Vec<u8>,
    issuer: Issuer<'static, KeyPair>,
    authority_expires_at: OffsetDateTime,
    leaf_expires_at: OffsetDateTime,
    config: Arc<ServerConfig>,
}

trait SecretStore {
    fn get(&self, account: &str) -> Result<Option<Vec<u8>>>;
    fn set(&self, account: &str, bytes: &[u8]) -> Result<()>;
}

struct KeychainStore(String);
impl SecretStore for KeychainStore {
    fn get(&self, account: &str) -> Result<Option<Vec<u8>>> {
        #[cfg(target_os = "macos")]
        match security_framework::passwords::get_generic_password(&self.0, account) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.code() == -25300 => Ok(None), // errSecItemNotFound only
            Err(error) => Err(anyhow::anyhow!(
                "Unlock Keychain and retry Choro Remote: {error}"
            )),
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = account;
            bail!("Choro TLS identity requires macOS Keychain")
        }
    }

    fn set(&self, account: &str, bytes: &[u8]) -> Result<()> {
        #[cfg(target_os = "macos")]
        {
            security_framework::passwords::set_generic_password(&self.0, account, bytes)
                .context("Could not save Choro TLS identity in Keychain; unlock Keychain and retry")
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (account, bytes);
            bail!("Choro TLS identity requires macOS Keychain")
        }
    }
}

impl TlsIdentity {
    pub fn load_default() -> Result<Self> {
        let store = ide_core::local_store::LocalStore::open_default()
            .context("Could not locate Choro's TLS identity directory")?;
        let path = store
            .app_data_dir()
            .join("remote")
            .join("tls-identity-v1.json");
        let service =
            std::env::var("CHORO_TLS_KEYCHAIN_SERVICE").unwrap_or_else(|_| KEYCHAIN_SERVICE.into());
        ensure!(
            !service.trim().is_empty(),
            "TLS Keychain service must not be empty"
        );
        Self::load(&path, &KeychainStore(service), OffsetDateTime::now_utc())
    }

    fn load(path: &Path, store: &impl SecretStore, now: OffsetDateTime) -> Result<Self> {
        let parent = path
            .parent()
            .context("TLS identity needs a data directory")?;
        fs::create_dir_all(parent)?;
        // Serialize first initialization across concurrent Desktop processes.
        // The small lock file remains in place; releasing a lock never deletes it.
        let _lock = lock_identity(&path.with_extension("lock"))?;
        let mut metadata = match fs::read(path) {
            Ok(bytes) => Some(
                serde_json::from_slice::<Metadata>(&bytes)
                    .context("TLS identity metadata is damaged; restore it before pairing")?,
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error).context("Cannot read TLS identity metadata"),
        };
        let account = metadata
            .as_ref()
            .map(|metadata| metadata.account.clone())
            .unwrap_or_else(|| {
                // Stable for this installation's data location even if public
                // metadata is accidentally removed. Such removal must not rotate CA.
                format!(
                    "installation-{:x}",
                    Sha256::digest(path.as_os_str().as_encoded_bytes())
                )
            });
        ensure!(
            account.len() == 77
                && account.starts_with("installation-")
                && account[13..].bytes().all(|byte| byte.is_ascii_hexdigit()),
            "Invalid TLS identity account metadata"
        );
        let existing = store.get(&account)?;
        let secret = match (metadata.as_ref(), existing) {
            (None, None) => {
                let reservation = Metadata { version: 1, account: account.clone(), authority_sha256: None };
                write_atomic(path, &serde_json::to_vec_pretty(&reservation)?)?;
                metadata = Some(reservation);
                let key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)?;
                let expires_at = now + Duration::days(3650);
                let certificate = authority_params(&account, now, expires_at).self_signed(&key)?;
                let secret = AuthoritySecret {
                    version: 1,
                    authority_der: STANDARD.encode(certificate.der()),
                    private_key_pkcs8: STANDARD.encode(key.serialize_der()),
                    created_at: now.unix_timestamp(),
                    expires_at: expires_at.unix_timestamp(),
                };
                store.set(&account, &serde_json::to_vec(&secret)?)?;
                secret
            }
            (Some(_), Some(bytes)) => serde_json::from_slice::<AuthoritySecret>(&bytes)
                .context("TLS identity in Keychain is damaged; restore it before pairing")?,
            (Some(_), None) => bail!("Choro's TLS identity is missing from Keychain. Restore or unlock the original identity, or explicitly reset pairing; no replacement was created."),
            (None, Some(_)) => bail!("Choro's TLS metadata is missing but its Keychain identity exists. Restore the metadata before pairing; no replacement was created."),
        };
        let mut metadata = metadata.context("Missing TLS initialization reservation")?;
        ensure!(
            metadata.version == 1 && secret.version == 1,
            "Unsupported TLS identity version"
        );
        let authority = STANDARD
            .decode(&secret.authority_der)
            .context("Invalid TLS authority")?;
        let fingerprint = format!("{:x}", Sha256::digest(&authority));
        ensure!(metadata.authority_sha256.as_ref().is_none_or(|expected| expected == &fingerprint), "TLS authority changed unexpectedly; restore the original identity or explicitly pair again");
        let created_at = OffsetDateTime::from_unix_timestamp(secret.created_at)?;
        let expires_at = OffsetDateTime::from_unix_timestamp(secret.expires_at)?;
        ensure!(
            expires_at > now + Duration::days(1),
            "Choro's TLS authority has expired; explicitly renew its identity and pair again"
        );
        let key = KeyPair::from_pkcs8_der_and_sign_algo(
            &PrivatePkcs8KeyDer::from(STANDARD.decode(&secret.private_key_pkcs8)?),
            &rcgen::PKCS_ECDSA_P256_SHA256,
        )
        .context("Invalid TLS authority key")?;
        // Certificate construction parameters are versioned above. No hand-built
        // DER or certificate parser is used: rcgen constructs, rustls verifies.
        let issuer = Issuer::new(authority_params(&account, created_at, expires_at), key);
        let (config, leaf_expires_at) = issue_leaf(&issuer, &authority, now, expires_at)?;
        if metadata.authority_sha256.is_none() {
            metadata.authority_sha256 = Some(fingerprint);
            write_atomic(path, &serde_json::to_vec_pretty(&metadata)?)?;
        }
        Ok(Self {
            authority,
            issuer,
            authority_expires_at: expires_at,
            leaf_expires_at,
            config,
        })
    }

    pub fn authority_der(&self) -> &[u8] {
        &self.authority
    }

    pub fn server_config(&mut self) -> Result<Arc<ServerConfig>> {
        self.server_config_at(OffsetDateTime::now_utc())
    }

    fn server_config_at(&mut self, now: OffsetDateTime) -> Result<Arc<ServerConfig>> {
        ensure!(
            self.authority_expires_at > now + Duration::days(1),
            "Choro's TLS authority has expired; explicitly pair again"
        );
        if self.leaf_expires_at <= now + Duration::days(RENEW_BEFORE_DAYS) {
            let (config, expires_at) = issue_leaf(
                &self.issuer,
                &self.authority,
                now,
                self.authority_expires_at,
            )?;
            self.config = config;
            self.leaf_expires_at = expires_at;
        }
        Ok(self.config.clone())
    }
}

fn authority_params(
    account: &str,
    created_at: OffsetDateTime,
    expires_at: OffsetDateTime,
) -> CertificateParams {
    let mut params = CertificateParams::default();
    params.distinguished_name.push(
        DnType::CommonName,
        format!(
            "Choro Remote {}",
            account.chars().take(40).collect::<String>()
        ),
    );
    params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
    params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    params.not_before = created_at - Duration::minutes(5);
    params.not_after = expires_at;
    params
}

fn issue_leaf(
    issuer: &Issuer<'_, KeyPair>,
    authority: &[u8],
    now: OffsetDateTime,
    authority_expires: OffsetDateTime,
) -> Result<(Arc<ServerConfig>, OffsetDateTime)> {
    let mut params = CertificateParams::new(vec![TLS_HOSTNAME.to_owned()])?;
    params
        .distinguished_name
        .push(DnType::CommonName, "Choro Desktop");
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    params.not_before = now - Duration::minutes(5);
    let expires_at = (now + Duration::days(365)).min(authority_expires);
    params.not_after = expires_at;
    let key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)?;
    let leaf = params.signed_by(&key, issuer)?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut roots = RootCertStore::empty();
    roots.add(CertificateDer::from(authority.to_vec()))?;
    let verifier = rustls::client::WebPkiServerVerifier::builder_with_provider(
        Arc::new(roots),
        provider.clone(),
    )
    .build()?;
    verifier
        .verify_server_cert(
            leaf.der(),
            &[],
            &ServerName::try_from(TLS_HOSTNAME)?,
            &[],
            UnixTime::since_unix_epoch(std::time::Duration::from_secs(
                now.unix_timestamp().try_into()?,
            )),
        )
        .context("TLS authority and Keychain key do not match or are invalid")?;
    let mut config = ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .with_no_client_auth()
        .with_single_cert(
            vec![leaf.der().clone()],
            PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
        )?;
    config.alpn_protocols = vec![TLS_ALPN.to_vec()];
    config.max_early_data_size = 0;
    config.send_tls13_tickets = 0;
    Ok((Arc::new(config), expires_at))
}

fn lock_identity(path: &Path) -> Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.create(true).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd as _;
        // SAFETY: file owns a live descriptor for the entire lock lifetime.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(std::io::Error::last_os_error())
                .context("Another Choro process is initializing TLS; retry shortly");
        }
    }
    Ok(file)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let temporary = path.with_extension(format!("tmp-{}", Uuid::new_v4().simple()));
    let mut options = fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(temporary.clone())?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(temporary, path)?;
    if let Some(parent) = path.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    #[derive(Default)]
    struct MemoryStore {
        value: RefCell<Option<Vec<u8>>>,
        inaccessible: RefCell<bool>,
    }
    impl SecretStore for MemoryStore {
        fn get(&self, _: &str) -> Result<Option<Vec<u8>>> {
            ensure!(!*self.inaccessible.borrow(), "Keychain locked");
            Ok(self.value.borrow().clone())
        }
        fn set(&self, _: &str, bytes: &[u8]) -> Result<()> {
            ensure!(!*self.inaccessible.borrow(), "Keychain locked");
            self.value.replace(Some(bytes.to_vec()));
            Ok(())
        }
    }

    #[test]
    fn identity_survives_reload_and_renews_under_the_original_authority() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("identity.json");
        let store = MemoryStore::default();
        let now = OffsetDateTime::now_utc();
        let mut first = TlsIdentity::load(&path, &store, now).unwrap();
        let old_config = first.server_config_at(now).unwrap();
        let renewed = first.server_config_at(now + Duration::days(340)).unwrap();
        assert!(!Arc::ptr_eq(&old_config, &renewed));
        let reloaded = TlsIdentity::load(&path, &store, now).unwrap();
        assert_eq!(first.authority_der(), reloaded.authority_der());
        let metadata = fs::read_to_string(path).unwrap();
        assert!(!metadata.contains("private_key"));
        assert_eq!(renewed.alpn_protocols, vec![TLS_ALPN.to_vec()]);
        assert_eq!(renewed.send_tls13_tickets, 0);
    }

    #[test]
    fn missing_or_locked_keychain_never_replaces_the_identity() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("identity.json");
        let store = MemoryStore::default();
        let now = OffsetDateTime::now_utc();
        TlsIdentity::load(&path, &store, now).unwrap();
        let metadata = fs::read(&path).unwrap();
        *store.inaccessible.borrow_mut() = true;
        assert!(TlsIdentity::load(&path, &store, now).is_err());
        *store.inaccessible.borrow_mut() = false;
        store.value.replace(None);
        assert!(TlsIdentity::load(&path, &store, now).is_err());
        assert_eq!(fs::read(&path).unwrap(), metadata);
        assert!(store.value.borrow().is_none());
    }

    #[test]
    fn mismatched_key_and_authority_are_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("identity.json");
        let store = MemoryStore::default();
        let now = OffsetDateTime::now_utc();
        TlsIdentity::load(&path, &store, now).unwrap();
        let mut secret: AuthoritySecret =
            serde_json::from_slice(store.value.borrow().as_ref().unwrap()).unwrap();
        secret.private_key_pkcs8 = STANDARD.encode(
            KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)
                .unwrap()
                .serialize_der(),
        );
        store
            .value
            .replace(Some(serde_json::to_vec(&secret).unwrap()));
        assert!(TlsIdentity::load(&path, &store, now).is_err());
    }
}
