//! Bounded image downloads for Studio. Sources never choose a destination path.
use anyhow::{anyhow, ensure, Context, Result};
use std::io::Read;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::path::Path;
use std::time::Duration;

const LIMIT: u64 = 4 * 1024 * 1024;

pub fn read_studio_asset(source: &str, allow_figma_local: bool) -> Result<Vec<u8>> {
    if source.starts_with("https://") || source.starts_with("http://") {
        download(source, allow_figma_local)
    } else {
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)] {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NONBLOCK);
        }
        let file = options.open(source).context("Could not open the image file")?;
        ensure!(file.metadata()?.is_file(), "Choose an image file");
        bounded(file)
    }
}

pub fn validate_studio_import(name: &str, bytes: &[u8]) -> Result<()> {
    ensure!(name.len() <= 160 && Path::new(name).file_name().and_then(|v| v.to_str()) == Some(name)
        && !name.contains(['/', '\\']), "Use a plain asset filename");
    let extension = Path::new(name).extension().and_then(|v| v.to_str()).unwrap_or("").to_ascii_lowercase();
    let valid = match extension.as_str() {
        "png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "jpg" | "jpeg" => bytes.starts_with(b"\xff\xd8\xff"),
        "gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        "webp" => bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"),
        "woff" => bytes.starts_with(b"wOFF"),
        "woff2" => bytes.starts_with(b"wOF2"),
        _ => false,
    };
    ensure!(valid && !bytes.is_empty() && bytes.len() as u64 <= LIMIT,
        "Import a PNG, JPEG, GIF, WebP, WOFF or WOFF2 file under 4 MB, with the matching filename extension");
    Ok(())
}

fn bounded(reader: impl Read) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(LIMIT + 1).read_to_end(&mut bytes)?;
    ensure!(!bytes.is_empty() && bytes.len() as u64 <= LIMIT, "Asset must be between 1 byte and 4 MB");
    Ok(bytes)
}

fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => !ip.is_private() && !ip.is_loopback() && !ip.is_link_local()
            && !ip.is_unspecified() && !ip.is_broadcast() && !ip.is_multicast()
            && ip.octets()[0] != 0 && ip.octets()[0] < 224
            && !(ip.octets()[0] == 100 && (64..=127).contains(&ip.octets()[1])),
        IpAddr::V6(ip) => ip.to_ipv4_mapped().map(|v| public_ip(v.into())).unwrap_or_else(||
            !ip.is_loopback() && !ip.is_unspecified() && !ip.is_multicast()
                && (ip.segments()[0] & 0xe000) == 0x2000),
    }
}

fn source_addresses(url: &url::Url, allow_figma_local: bool) -> Result<Vec<SocketAddr>> {
    ensure!(url.username().is_empty() && url.password().is_none(), "Asset URLs cannot contain login credentials");
    let host = url.host_str().context("Missing asset host")?;
    let port = url.port_or_known_default().context("Unsupported asset URL")?;
    let figma_local = allow_figma_local && url.scheme() == "http" && port == 3845
        && matches!(host, "localhost" | "127.0.0.1") && url.path().starts_with("/assets/");
    ensure!(url.scheme() == "https" || figma_local, "Use an HTTPS image URL or a connected Figma desktop asset");
    let addresses: Vec<_> = (host.trim_matches(['[', ']']), port).to_socket_addrs()
        .map_err(|_| anyhow!("Could not resolve the asset host"))?.collect();
    ensure!(!addresses.is_empty() && addresses.iter().all(|address| public_ip(address.ip())
        || (figma_local && address.ip().is_loopback())), "Asset URLs cannot access private network addresses");
    Ok(addresses)
}

fn download(source: &str, allow_figma_local: bool) -> Result<Vec<u8>> {
    let mut url = url::Url::parse(source).map_err(|_| anyhow!("Invalid asset URL"))?;
    for _ in 0..5 {
        let addresses = source_addresses(&url, allow_figma_local)?;
        let host = url.host_str().context("Missing asset host")?;
        let client = reqwest::blocking::Client::builder().no_proxy()
            .redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10)).resolve_to_addrs(host, &addresses).build()?;
        let response = client.get(url.clone()).send().map_err(|_| anyhow!("Could not download the asset"))?;
        if response.status().is_redirection() {
            let location = response.headers().get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok()).context("Asset redirect has no destination")?;
            url = url.join(location).map_err(|_| anyhow!("Invalid asset redirect"))?;
            continue;
        }
        ensure!(response.status().is_success(), "Asset download failed with HTTP {}", response.status().as_u16());
        ensure!(response.content_length().is_none_or(|size| size <= LIMIT), "Asset exceeds 4 MB");
        return bounded(response);
    }
    anyhow::bail!("Asset redirected too many times")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn local_asset_sources_reject_pipes_without_waiting_for_a_writer() {
        use std::os::unix::ffi::OsStrExt;
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("image.png");
        let name = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        let started = std::time::Instant::now();
        assert!(read_studio_asset(path.to_str().unwrap(), false).is_err());
        assert!(started.elapsed() < Duration::from_secs(1));
    }
    #[test]
    fn rejects_private_and_credential_bearing_sources_and_false_image_files() {
        for url in ["https://127.0.0.1/image.png", "http://localhost:3845/private", "https://user:secret@example.com/image.png"] {
            assert!(source_addresses(&url::Url::parse(url).unwrap(), true).is_err());
        }
        for ip in ["10.1.2.3", "127.0.0.1", "169.254.169.254", "100.64.0.1", "::ffff:127.0.0.1", "fd00::1"] {
            assert!(!public_ip(ip.parse().unwrap()), "{ip}");
        }
        assert!(validate_studio_import("secret.png", b"secret text").is_err());
        assert!(validate_studio_import("../image.png", b"\x89PNG\r\n\x1a\n").is_err());
        assert!(validate_studio_import("image.png", b"\x89PNG\r\n\x1a\n").is_ok());
        assert!(validate_studio_import("image.svg", b"<svg><script/></svg>").is_err());
    }
}
