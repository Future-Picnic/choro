//! Finder-launched apps normally discard stderr. Preserve errors without
//! intercepting a developer's terminal, pipe, or explicit log redirection.
pub(crate) fn initialize() {
    #[cfg(unix)]
    if let Err(error) = capture_discarded_stderr() {
        eprintln!("Could not initialize Choro diagnostics: {error:#}");
    }
}

#[cfg(unix)]
fn capture_discarded_stderr() -> anyhow::Result<()> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let null = std::fs::metadata("/dev/null")?;
    let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
    // fstat initializes the buffer on success; dup2 atomically replaces only
    // stderr, after the destination is open with private permissions.
    unsafe {
        if libc::fstat(libc::STDERR_FILENO, metadata.as_mut_ptr()) != 0 {
            return Ok(());
        }
        let metadata = metadata.assume_init();
        if metadata.st_rdev as u64 != null.rdev()
            || metadata.st_mode & libc::S_IFMT != libc::S_IFCHR
        {
            return Ok(());
        }
    }
    let directory = ide_core::AppConfig::config_root().join("logs");
    std::fs::create_dir_all(&directory)?;
    let path = directory.join(format!(
        "choro-{}-{}.log",
        ide_core::agents::unix_now(),
        std::process::id()
    ));
    let file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)?;
    if unsafe { libc::dup2(file.as_raw_fd(), libc::STDERR_FILENO) } < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    eprintln!(
        "Choro session started at {} (pid {})",
        ide_core::agents::unix_now(),
        std::process::id()
    );
    Ok(())
}
