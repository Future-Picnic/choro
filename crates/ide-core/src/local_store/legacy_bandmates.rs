//! Explicit, restartable cleanup of retired delegation copies. Never used by
//! ordinary task startup, completion, export, or an agent tool.
use super::*;
use anyhow::ensure;

#[cfg(any(target_os = "macos", target_os = "linux"))]
mod unix_cleanup;

#[derive(Clone, Debug, Serialize)]
pub struct LegacyBandmateStoragePreview {
    pub root: PathBuf,
    pub allocated_bytes: u64,
    pub files: u64,
}

fn validate_root(app_root: &Path) -> Result<PathBuf> {
    ensure!(!fs::symlink_metadata(app_root)?.is_symlink(), "Application storage cannot be a symlink.");
    let app_root = app_root.canonicalize()?;
    let data = app_root.join("data");
    if data.exists() {
        ensure!(fs::symlink_metadata(&data)?.is_dir() && !fs::symlink_metadata(&data)?.is_symlink(), "Application data cannot be a symlink.");
    }
    let root = data.join("delegation");
    if let Ok(meta) = fs::symlink_metadata(&root) {
        ensure!(meta.is_dir() && !meta.is_symlink(), "Refusing a linked or non-directory delegation root.");
    }
    Ok(root)
}

fn measure(path: &Path, preview: &mut LegacyBandmateStoragePreview) -> Result<()> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    #[cfg(unix)]
    { use std::os::unix::fs::MetadataExt; preview.allocated_bytes += meta.blocks() * 512; }
    #[cfg(not(unix))]
    { preview.allocated_bytes += meta.len(); }
    if meta.is_dir() && !meta.is_symlink() {
        for entry in fs::read_dir(path)? { measure(&entry?.path(), preview)?; }
    } else { preview.files += 1; }
    Ok(())
}

/// Filesystem metadata only: no store opening, schema migration, or writes.
pub fn preview_legacy_bandmate_storage(app_root: &Path) -> Result<LegacyBandmateStoragePreview> {
    let root = validate_root(app_root)?;
    let mut preview = LegacyBandmateStoragePreview { root: root.clone(), allocated_bytes: 0, files: 0 };
    measure(&root, &mut preview)?;
    Ok(preview)
}

/// Old binaries do not know the new stop gate. Refuse cleanup while any
/// database user or process rooted in a legacy copy remains alive.
pub fn require_legacy_storage_offline(app_root: &Path) -> Result<()> {
    let root = validate_root(app_root)?;
    let db = app_root.canonicalize()?.join("state.db");
    // An idle Turso client may hold no open database descriptor. Old binaries
    // do not obey new locks, so their process identity must also be absent.
    let processes = Command::new("ps").args(["-axo", "pid=,comm="]).output()?;
    ensure!(processes.status.success(), "Could not check Choro processes");
    for line in String::from_utf8(processes.stdout)?.lines() {
        let line = line.trim();
        let Some((pid, command)) = line.split_once(char::is_whitespace) else { continue; };
        if pid.parse::<u32>().ok() == Some(std::process::id()) { continue; }
        ensure!(!is_legacy_app_process(command.trim()), "Close Choro before cleanup (process {pid} can reopen its database). No files were removed.");
    }
    for args in [vec!["-Fpcn".to_owned()]] {
        let output = Command::new("lsof").args(&args).output().context("lsof is required to verify cleanup is offline")?;
        ensure!(matches!(output.status.code(), Some(0 | 1)), "Could not establish whether Choro storage is in use.");
        let text = String::from_utf8(output.stdout)?;
        let mut process = String::new();
        for line in text.lines() {
            if let Some(pid) = line.strip_prefix('p') { process = pid.to_owned(); }
            if let Some(name) = line.strip_prefix('n') {
                let path = Path::new(name);
                if process.parse::<u32>().ok() == Some(std::process::id()) { continue; }
                ensure!(path != db && !path.starts_with(&root), "Cleanup requires Choro and old Bandmate processes to be closed (process {process} is still using storage). No files were removed.");
            }
        }
    }
    Ok(())
}

fn is_legacy_app_process(command: &str) -> bool {
    let name = Path::new(command).file_name().and_then(|s| s.to_str()).unwrap_or_default();
    matches!(name.to_ascii_lowercase().as_str(), "choro" | "choro-mcp" | "ide-app" | "ide-mcp")
}

fn remove_contents(root: &Path) -> Result<()> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    return remove_tree_with_retry(root, unix_cleanup::remove_snapshot_tree);
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    remove_tree_with_retry(root, |path| fs::remove_dir_all(path))
}

fn remove_tree_with_retry(
    root: &Path,
    mut remove: impl FnMut(&Path) -> std::io::Result<()>,
) -> Result<()> {
    const MAX_PASSES: usize = 8;
    for pass in 0..MAX_PASSES {
        let meta = match fs::symlink_metadata(root) {
            Ok(meta) => meta,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error).with_context(|| format!("Could not inspect cleanup root {}", root.display())),
        };
        ensure!(meta.is_dir() && !meta.is_symlink(), "Refusing a linked or non-directory cleanup root: {}", root.display());
        // Each pass snapshots directory entries before deleting them. A later
        // pass handles entries added during removal; retries stay bounded.
        match remove(root) {
            Ok(()) => return Ok(()),
            Err(error) if matches!(error.kind(), std::io::ErrorKind::DirectoryNotEmpty | std::io::ErrorKind::NotFound) => {
                // NotFound can refer to a child, so it is not proof that the
                // approved root was removed. Check it again on the next pass.
                match fs::symlink_metadata(root) {
                    Err(missing) if missing.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                    Ok(meta) => ensure!(meta.is_dir() && !meta.is_symlink(), "Cleanup root changed to a link or non-directory: {}", root.display()),
                    Err(inspect) => return Err(inspect).with_context(|| format!("Could not inspect remaining cleanup root {}", root.display())),
                }
                if pass + 1 == MAX_PASSES {
                    let remaining = fs::read_dir(root)?
                        .take(6)
                        .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
                        .collect::<std::io::Result<Vec<_>>>()?;
                    return Err(error).with_context(|| format!(
                        "Cleanup stopped at {} after {MAX_PASSES} passes. Remaining entries include: {}. Already removed copies stay removed; stop any process adding entries here before retrying",
                        root.display(), remaining.join(", ")
                    ));
                }
                eprintln!("Retrying legacy cleanup at {} (pass {}/{MAX_PASSES})", root.display(), pass + 2);
                std::thread::sleep(std::time::Duration::from_millis(100 * (pass as u64 + 1)));
            }
            Err(error) => return Err(error).with_context(|| format!("Could not remove legacy copies at {}", root.display())),
        }
    }
    unreachable!("each cleanup pass either succeeds, retries, or returns its error")
}

/// Call only after the user approves the preview and the offline preflight.
/// The public entry point rechecks processes before opening the database.
pub fn cleanup_legacy_bandmate_storage(app_root: &Path) -> Result<LegacyBandmateStoragePreview> {
    cleanup_with_checks(app_root, require_legacy_storage_offline, remove_contents)
}

fn cleanup_with_checks(
    app_root: &Path,
    offline: impl Fn(&Path) -> Result<()>,
    remove: impl Fn(&Path) -> Result<()>,
) -> Result<LegacyBandmateStoragePreview> {
    offline(app_root)?;
    let before = preview_legacy_bandmate_storage(app_root)?;
    let store = LocalStore::open(app_root.canonicalize()?)?;
    store.rt.block_on(async {
        let conn = store.connect().await?;
        execute_transaction(&conn, |conn| Box::pin(async move {
            super::delegation::close_legacy_async(conn).await?;
            set_meta(conn, "legacy_bandmate_cleanup", "pending").await?;
            Ok(())
        })).await
    })?;
    // The migration is durable before any filesystem removal. On failure,
    // pending records plus the remaining directory make retry idempotent.
    let root = validate_root(store.root())?;
    ensure!(root == before.root, "Cleanup root changed after preview.");
    let result = offline(app_root).and_then(|_| remove(&root));
    store.rt.block_on(async {
        let conn = store.connect().await?;
        set_meta(&conn, "legacy_bandmate_cleanup", &match &result {
            Ok(()) => "complete".into(),
            Err(error) => format!("incomplete: {error:#}"),
        }).await
    })?;
    result?;
    Ok(before)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nonempty_directory_retries_remaining_contents_after_partial_removal() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("delegation");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("removed-first"), b"copy").unwrap();
        fs::write(dir.path().join("keep"), b"project").unwrap();
        let mut passes = 0;
        remove_tree_with_retry(&root, |path| {
            passes += 1;
            if passes == 1 {
                fs::remove_file(path.join("removed-first"))?;
                fs::write(path.join(".DS_Store"), b"late metadata")?;
                return Err(std::io::ErrorKind::DirectoryNotEmpty.into());
            }
            fs::remove_dir_all(path)
        }).unwrap();
        assert_eq!(passes, 2);
        assert!(!root.exists());
        assert_eq!(fs::read(dir.path().join("keep")).unwrap(), b"project");
    }

    #[test]
    fn child_not_found_does_not_claim_the_root_was_removed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("delegation");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("remaining"), b"copy").unwrap();
        let mut passes = 0;
        remove_tree_with_retry(&root, |path| {
            passes += 1;
            if passes == 1 { return Err(std::io::ErrorKind::NotFound.into()); }
            fs::remove_dir_all(path)
        }).unwrap();
        assert_eq!(passes, 2);
        assert!(!root.exists());
    }

    #[test]
    fn exhausted_nonempty_retries_name_the_remaining_root_and_can_resume() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap().join("delegation");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("remaining"), b"copy").unwrap();
        let mut passes = 0;
        let error = remove_tree_with_retry(&root, |_| {
            passes += 1;
            Err(std::io::ErrorKind::DirectoryNotEmpty.into())
        }).unwrap_err();
        assert_eq!(passes, 8);
        assert!(format!("{error:#}").contains(&root.display().to_string()));
        assert!(format!("{error:#}").contains("remaining"));
        remove_contents(&root).unwrap();
        assert!(!root.exists());
    }

    #[cfg(unix)]
    #[test]
    fn root_replaced_by_a_symlink_between_passes_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let root = dir.path().join("delegation");
        fs::create_dir(&root).unwrap();
        fs::write(other.path().join("keep"), b"project").unwrap();
        let error = remove_tree_with_retry(&root, |path| {
            fs::remove_dir(path)?;
            std::os::unix::fs::symlink(other.path(), path)?;
            Err(std::io::ErrorKind::DirectoryNotEmpty.into())
        }).unwrap_err();
        assert!(format!("{error:#}").contains("link"));
        assert_eq!(fs::read(other.path().join("keep")).unwrap(), b"project");
    }

    #[test]
    fn offline_gate_recognizes_idle_choro_and_mcp_but_not_test_runners() {
        assert!(is_legacy_app_process("/Applications/Choro.app/Contents/MacOS/choro"));
        assert!(is_legacy_app_process("/Applications/Choro.app/Contents/MacOS/choro-mcp"));
        assert!(!is_legacy_app_process("/usr/bin/node"));
        assert!(!is_legacy_app_process("/tmp/target/debug/deps/choro-abcd1234"));
    }

    #[test]
    fn preview_and_removal_are_scoped_and_restartable() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap().join("data/delegation");
        fs::create_dir_all(root.join("orphan/blobs")).unwrap();
        fs::write(root.join("orphan/blobs/content"), b"old copy").unwrap();
        fs::write(dir.path().join("user.txt"), b"keep").unwrap();
        let preview = preview_legacy_bandmate_storage(dir.path()).unwrap();
        assert_eq!(preview.files, 1);
        assert!(root.join("orphan/blobs/content").exists());
        remove_contents(&root).unwrap();
        remove_contents(&root).unwrap();
        assert_eq!(fs::read(dir.path().join("user.txt")).unwrap(), b"keep");
        assert_eq!(preview_legacy_bandmate_storage(dir.path()).unwrap().files, 0);
    }
    #[cfg(unix)]
    #[test]
    fn links_never_expand_cleanup_scope() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        fs::write(other.path().join("keep"), b"user").unwrap();
        fs::create_dir(dir.path().join("data")).unwrap();
        let root = dir.path().canonicalize().unwrap().join("data/delegation");
        symlink(other.path(), &root).unwrap();
        assert!(preview_legacy_bandmate_storage(dir.path()).is_err());
        fs::remove_file(&root).unwrap();
        fs::create_dir(&root).unwrap();
        symlink(other.path(), root.join("linked-child")).unwrap();
        remove_contents(&root).unwrap();
        assert_eq!(fs::read(other.path().join("keep")).unwrap(), b"user");
    }
    #[test]
    fn interrupted_cleanup_records_failure_and_resumes_after_migration_commit() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("data/delegation");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("first"), b"first").unwrap();
        fs::write(root.join("remaining"), b"remaining").unwrap();
        let failed = cleanup_with_checks(dir.path(), |_| Ok(()), |root| {
            let store = LocalStore::open(dir.path().to_path_buf())?;
            let marker = store.rt.block_on(async { get_meta(&store.connect().await?, "legacy_bandmate_cleanup").await })?;
            assert_eq!(marker.as_deref(), Some("pending"));
            fs::remove_file(root.join("first"))?;
            anyhow::bail!("simulated interruption");
        });
        assert!(failed.is_err());
        assert!(root.join("remaining").exists());
        let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
        let marker = store.rt.block_on(async { get_meta(&store.connect().await?, "legacy_bandmate_cleanup").await }).unwrap();
        assert!(marker.unwrap().contains("incomplete"));
        cleanup_with_checks(dir.path(), |_| Ok(()), remove_contents).unwrap();
        assert!(!root.exists());
        let marker = store.rt.block_on(async { get_meta(&store.connect().await?, "legacy_bandmate_cleanup").await }).unwrap();
        assert_eq!(marker.as_deref(), Some("complete"));
        assert_eq!(cleanup_with_checks(dir.path(), |_| Ok(()), remove_contents).unwrap().allocated_bytes, 0);
    }

    #[test]
    fn failed_offline_check_cannot_migrate_or_delete_storage() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("data/delegation");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("keep"), b"keep").unwrap();
        assert!(cleanup_with_checks(dir.path(), |_| anyhow::bail!("runtime active"), remove_contents).is_err());
        assert!(root.join("keep").exists());
        assert!(!dir.path().join("state.db").exists());
    }

}
