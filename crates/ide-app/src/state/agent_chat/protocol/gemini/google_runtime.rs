//! Official Google ACP distribution, published in the ACP registry.
use super::*;
use std::io::Read as _;

const VERSION: &str = "1.2.1";

pub(super) fn profile_root() -> PathBuf {
    AppConfig::config_root().join("data/providers/google")
}

pub(super) fn cached_executable() -> Option<PathBuf> {
    std::env::var_os("GEMINI_ACP_CLI")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .or_else(|| find_executable(executable_name()))
        .or_else(|| {
            let path = profile_root()
                .join("bin")
                .join(VERSION)
                .join(executable_name());
            path.is_file().then_some(path)
        })
}

fn executable_name() -> &'static str {
    if cfg!(windows) {
        "agy_acp_server.exe"
    } else {
        "agy_acp_server.par"
    }
}

fn archive_url() -> anyhow::Result<String> {
    let platform = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "macos/agy-acp-server-1.2.1-darwin-arm64.zip",
        ("macos", "x86_64") => "macos/agy-acp-server-1.2.1-darwin-x86_64.zip",
        ("linux", "aarch64") => "linux/agy-acp-server-1.2.1-linux-arm64.zip",
        ("linux", "x86_64") => "linux/agy-acp-server-1.2.1-linux-x86_64.zip",
        ("windows", "aarch64") => "windows/agy-acp-server-1.2.1-windows-arm64.zip",
        ("windows", "x86_64") => "windows/agy-acp-server-1.2.1-windows-x86_64.zip",
        _ => {
            return Err(anyhow!(
                "Google's Gemini ACP server does not support this platform"
            ))
        }
    };
    Ok(format!(
        "https://dl.google.com/agy-extensions/releases/{platform}"
    ))
}

/// Download only into Choro's provider cache, on the backend worker.
pub(super) fn executable() -> anyhow::Result<PathBuf> {
    static INSTALL: Mutex<()> = Mutex::new(());
    let _install = INSTALL
        .lock()
        .map_err(|_| anyhow!("Google provider installer lock poisoned"))?;
    if let Some(path) = cached_executable() {
        return Ok(path);
    }
    const MAX_ARCHIVE: u64 = 256 * 1024 * 1024;
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(180))
        .build()?;
    let response = client.get(archive_url()?).send()?.error_for_status()?;
    let mut bytes = Vec::new();
    response.take(MAX_ARCHIVE + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_ARCHIVE,
        "Google provider download exceeds the size limit"
    );
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
    // Publish a unique complete directory so concurrent readers never see a
    // half-extracted provider or sidecar. No existing provider is overwritten.
    let bin_root = profile_root().join("bin");
    fs::create_dir_all(&bin_root)?;
    let staging = bin_root.join(format!("{VERSION}-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&staging)?;
    for name in [
        executable_name(),
        if cfg!(windows) {
            "localharness_external.exe"
        } else {
            "localharness_external"
        },
    ] {
        let mut member = archive
            .by_name(name)
            .with_context(|| format!("Google provider archive is missing {name}"))?;
        anyhow::ensure!(
            member.size() <= 512 * 1024 * 1024,
            "Google provider archive member exceeds the size limit"
        );
        let path = staging.join(name);
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        std::io::copy(&mut member, &mut file)?;
        file.sync_all()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        }
    }
    let published = bin_root.join(VERSION);
    fs::rename(&staging, &published).context("Could not publish Google's ACP server")?;
    Ok(published.join(executable_name()))
}

pub(super) fn command(executable: &Path) -> anyhow::Result<Command> {
    static SETTINGS: Mutex<()> = Mutex::new(());
    let _settings = SETTINGS
        .lock()
        .map_err(|_| anyhow!("Google provider settings lock poisoned"))?;
    let settings_dir = profile_root().join("antigravity-acp");
    fs::create_dir_all(&settings_dir)?;
    let settings_path = settings_dir.join("settings.json");
    if !settings_path.exists() {
        let mut settings = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(settings_path)?;
        // Explicit Ask rules take priority over Google's automatic workspace
        // writes. Choro resolves the resulting ACP permission requests.
        serde_json::to_writer(
            &mut settings,
            &json!({
                    "permissions": { "ask": ["read_file(*)", "write_file(*)", "command(*)", "unsandboxed(*)", "mcp(*)", "execute_url(*)", "read_url(*)"] }
            }),
        )?;
        settings.flush()?;
    }
    let mut command = Command::new(executable);
    command
        .env("GEMINI_HOME", profile_root())
        .env("PATH", command_path_env())
        .env_remove("GEMINI_API_KEY")
        .env_remove("GOOGLE_API_KEY");
    #[cfg(target_os = "linux")]
    command.arg("--uid=");
    Ok(command)
}
