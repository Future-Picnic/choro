use super::*;
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Component, Path},
};

pub const MAX_FILES: usize = 512;
pub const MAX_PACKAGE_BYTES: usize = 4 * 1024 * 1024;

pub fn validate_files(files: &BTreeMap<String, FrozenSkillFile>) -> Result<()> {
    ensure!(
        files.len() <= MAX_FILES,
        "Skill package has too many supporting files."
    );
    let mut total = 0usize;
    for (path, file) in files {
        ensure!(
            !path.is_empty()
                && !path.contains('\\')
                && !path.contains(':')
                && Path::new(path)
                    .components()
                    .all(|c| matches!(c, Component::Normal(_))),
            "Skill package contains an unsafe resource path."
        );
        ensure!(
            !file.content.contains('\0') && file.content.len() <= 512_000,
            "Skill resource is too large or is not supported text: {path}"
        );
        total = total
            .checked_add(file.content.len())
            .context("Skill package size overflow")?;
        ensure!(
            total <= MAX_PACKAGE_BYTES,
            "Skill resources exceed the 4 MiB package limit."
        );
    }
    Ok(())
}

fn collect(
    root: &Path,
    path: &Path,
    files: &mut BTreeMap<String, FrozenSkillFile>,
    visited: &mut usize,
) -> Result<()> {
    *visited += 1;
    ensure!(
        *visited <= 2048 && path.strip_prefix(root)?.components().count() <= 32,
        "Skill resources contain too many directories or are nested too deeply."
    );
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        !metadata.file_type().is_symlink(),
        "Skill resources contain a symlink at {}. Use a self-contained skill package.",
        path.display()
    );
    if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            if matches!(
                entry.file_name().to_str(),
                Some("node_modules" | ".git" | "__pycache__" | ".DS_Store")
            ) {
                continue;
            }
            collect(root, &entry.path(), files, visited)?;
        }
    } else {
        ensure!(
            metadata.is_file() && metadata.len() <= 512_000,
            "Unsupported skill resource: {}",
            path.display()
        );
        let file = open_resource(path, false)?;
        let metadata = file.metadata()?;
        ensure!(metadata.is_file(), "Skill resource is not a regular file.");
        let content = read_resource(file, path)?;
        #[cfg(unix)]
        let executable = {
            use std::os::unix::fs::PermissionsExt;
            metadata.permissions().mode() & 0o111 != 0
        };
        #[cfg(not(unix))]
        let executable = false;
        files.insert(
            path.strip_prefix(root)?
                .to_str()
                .context("Skill paths must be valid Unicode")?
                .replace('\\', "/"),
            FrozenSkillFile {
                content,
                executable,
            },
        );
        validate_files(files)?;
    }
    Ok(())
}

fn capture_once(source: &Path) -> Result<BTreeMap<String, FrozenSkillFile>> {
    // Installation folders can be aliases, but the entrypoint itself must not
    // redirect a reviewed skill to an unrelated file during capture.
    let folder = source
        .parent()
        .context("Skill has no containing folder")?
        .canonicalize()?;
    let source = folder.join(source.file_name().context("Skill has no entrypoint name")?);
    let root = folder.as_path();
    let mut files = BTreeMap::new();
    let mut visited = 0;
    collect(root, &source, &mut files, &mut visited)?;
    // Capture supporting resources, never an arbitrary provider/plugin tree.
    for name in ["reference", "references", "rules", "scripts", "assets"] {
        let path = root.join(name);
        match fs::symlink_metadata(&path) {
            Ok(_) => collect(root, &path, &mut files, &mut visited)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    for entry in fs::read_dir(root)? {
        visited += 1;
        ensure!(
            visited <= 2048,
            "Skill package contains too many directory entries."
        );
        let path = entry?.path();
        if path != source
            && path
                .extension()
                .is_some_and(|e| matches!(e.to_str(), Some("md" | "txt" | "json" | "yaml" | "yml")))
            && !path.is_dir()
        {
            collect(root, &path, &mut files, &mut visited)?;
        }
    }
    // Legacy catalog entries can name a markdown entrypoint differently.
    let key = source
        .file_name()
        .and_then(|n| n.to_str())
        .context("Invalid skill filename")?;
    if key != "SKILL.md" {
        files.insert(
            "SKILL.md".into(),
            files.get(key).context("Missing skill entrypoint")?.clone(),
        );
    }
    Ok(files)
}

pub fn capture(source: &Path) -> Result<BTreeMap<String, FrozenSkillFile>> {
    for _ in 0..3 {
        let before = capture_once(source)?;
        if before == capture_once(source)? {
            return Ok(before);
        }
    }
    bail!("Skill files changed during capture. Retry when the skill installation is stable.")
}

pub fn materialize(root: &Path, files: &BTreeMap<String, FrozenSkillFile>) -> Result<PathBuf> {
    validate_files(files)?;
    ensure!(
        files.contains_key("SKILL.md"),
        "Frozen skill has no entrypoint."
    );
    fs::create_dir_all(root)?;
    let root = root.canonicalize()?;
    // Several Experts can use the same package at once, including across the
    // GUI and helper processes. Do not expose another writer's partial file.
    #[cfg(unix)]
    let _lease = CacheLease::acquire(&root)?;
    let digest = format!("{:x}", Sha256::digest(serde_json::to_vec(files)?));
    let base = root.join(digest);
    for (name, resource) in files {
        let target = base.join(name);
        match open_resource(&target, true) {
            Ok(mut file) => {
                file.write_all(resource.content.as_bytes())?;
                file.sync_all()?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    file.set_permissions(fs::Permissions::from_mode(if resource.executable {
                        0o500
                    } else {
                        0o400
                    }))?;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let file = open_resource(&target, false)?;
                let meta = file.metadata()?;
                ensure!(meta.is_file() && read_resource(file, &target)? == resource.content, "Frozen Bandmate skill cache changed at {}. Preserve it for inspection and repair the cache before retrying.", target.display());
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    ensure!(
                        (meta.permissions().mode() & 0o111 != 0) == resource.executable,
                        "Frozen skill executable permissions changed at {}.",
                        target.display()
                    );
                }
            }
            Err(e) => return Err(e.into()),
        }
    }
    Ok(base.join("SKILL.md"))
}

#[cfg(unix)]
struct CacheLease(fs::File);
#[cfg(unix)]
impl CacheLease {
    fn acquire(root: &Path) -> Result<Self> {
        use std::os::{fd::AsRawFd, unix::fs::OpenOptionsExt};
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(root.join(".materialize.lock"))?;
        ensure!(
            file.metadata()?.is_file(),
            "Bandmate cache lock is not a regular file."
        );
        // SAFETY: flock receives a live owned descriptor and valid operation.
        ensure!(
            unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } == 0,
            "Cannot lock Bandmate skill cache."
        );
        Ok(Self(file))
    }
}
#[cfg(unix)]
impl Drop for CacheLease {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;
        // SAFETY: descriptor remains owned until after this destructor.
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

fn read_resource(file: fs::File, path: &Path) -> Result<String> {
    let mut content = String::new();
    file.take(512_001)
        .read_to_string(&mut content)
        .with_context(|| format!("Skill resource is not supported text: {}", path.display()))?;
    ensure!(
        content.len() <= 512_000 && !content.contains('\0'),
        "Skill resource exceeds the text limit: {}",
        path.display()
    );
    Ok(content)
}

// Anchor every path component to an open directory descriptor. O_NOFOLLOW on
// the final file alone would still allow a replaced parent directory to escape.
#[cfg(unix)]
fn open_resource(path: &Path, create: bool) -> std::io::Result<fs::File> {
    use std::{
        ffi::CString,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::ffi::OsStrExt,
        },
    };
    let invalid = || {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Unsafe Bandmate skill resource path",
        )
    };
    if !path.is_absolute() {
        return Err(invalid());
    }
    let mut directory = fs::File::open("/")?;
    let components = path.components().skip(1).collect::<Vec<_>>();
    for (i, component) in components.iter().enumerate() {
        let Component::Normal(name) = component else {
            return Err(invalid());
        };
        let name = CString::new(name.as_bytes()).map_err(|_| invalid())?;
        let final_file = i + 1 == components.len();
        let flags = libc::O_CLOEXEC
            | libc::O_NOFOLLOW
            | libc::O_NONBLOCK
            | if final_file {
                if create {
                    libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL
                } else {
                    libc::O_RDONLY
                }
            } else {
                libc::O_RDONLY | libc::O_DIRECTORY
            };
        if create && !final_file {
            // SAFETY: directory owns its fd; name is a valid NUL-terminated component.
            let made = unsafe { libc::mkdirat(directory.as_raw_fd(), name.as_ptr(), 0o700) };
            if made != 0
                && std::io::Error::last_os_error().kind() != std::io::ErrorKind::AlreadyExists
            {
                return Err(std::io::Error::last_os_error());
            }
        }
        // SAFETY: openat uses a live directory fd, validated component and valid flags.
        let fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags, 0o600) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: successful openat transfers a new owned descriptor to this File.
        directory = unsafe { fs::File::from_raw_fd(fd) };
    }
    Ok(directory)
}

#[cfg(not(unix))]
fn open_resource(_path: &Path, _create: bool) -> std::io::Result<fs::File> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Frozen Bandmate packages require the supported macOS/Unix runtime.",
    ))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn open_rejects_replaced_parent_and_final_symlinks_for_reads_and_writes() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let root = root.path().canonicalize().unwrap();
        let outside = root.join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("secret.txt"), "private").unwrap();
        symlink(&outside, root.join("replaced-directory")).unwrap();
        assert!(open_resource(&root.join("replaced-directory/secret.txt"), false).is_err());
        assert!(open_resource(&root.join("replaced-directory/new.txt"), true).is_err());
        assert!(!outside.join("new.txt").exists());
        symlink(outside.join("secret.txt"), root.join("replaced-file")).unwrap();
        assert!(open_resource(&root.join("replaced-file"), false).is_err());
        assert!(open_resource(&root.join("replaced-file"), true).is_err());
        assert_eq!(
            fs::read_to_string(outside.join("secret.txt")).unwrap(),
            "private"
        );
    }
}
