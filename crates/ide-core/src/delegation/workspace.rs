//! Private working copies and recoverable three-way integration. Source Git
//! metadata is read only; checkpoints and commits belong to private copies.
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileKind {
    Regular,
    Symlink,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotFile {
    pub hash: String,
    pub kind: FileKind,
    pub executable: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspaceSnapshot {
    pub id: Uuid,
    pub source: PathBuf,
    pub storage: PathBuf,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub index_hash: Option<String>,
    pub files: BTreeMap<PathBuf, SnapshotFile>,
    pub excluded: Vec<PathBuf>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IntegrationChange {
    pub path: PathBuf,
    pub before: Option<SnapshotFile>,
    pub after: Option<SnapshotFile>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntegrationStatus {
    Prepared,
    Conflict,
    Applying,
    Applied,
    NeedsAttention,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IntegrationOperation {
    pub id: Uuid,
    pub source: PathBuf,
    pub storage: PathBuf,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub index_hash: Option<String>,
    pub changes: Vec<IntegrationChange>,
    pub conflicts: Vec<PathBuf>,
    pub resolution_dir: PathBuf,
    pub applied_paths: Vec<PathBuf>,
    pub status: IntegrationStatus,
    #[serde(default)]
    pub archive_snapshot: Option<WorkspaceSnapshot>,
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()?;
    ensure!(
        output.status.success(),
        "Git could not prepare this task: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(output.stdout)
}

fn source_identity(root: &Path) -> Result<(Option<String>, Option<String>, Option<String>)> {
    let repo =
        git2::Repository::open(root).context("Choose a Git repository for this assignment.")?;
    let head = repo.head().ok();
    let oid = head
        .as_ref()
        .and_then(|h| h.target())
        .map(|id| id.to_string());
    let branch = head
        .as_ref()
        .and_then(|h| h.name().ok())
        .map(str::to_string)
        .or_else(|| {
            repo.find_reference("HEAD")
                .ok()
                .and_then(|h| h.symbolic_target().ok().flatten().map(str::to_string))
        });
    let index = fs::read(repo.path().join("index"))
        .ok()
        .map(|bytes| hash(&bytes));
    Ok((oid, branch, index))
}

fn excluded(path: &Path) -> bool {
    path.components()
        .any(|part| matches!(part, Component::Normal(name) if name == ".git"))
        || crate::git::is_internal_visualization_path(path)
        || crate::git::is_generated_tool_path(path)
        || path.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
            n == ".env"
                || (n.starts_with(".env.") && !n.ends_with("example") && !n.ends_with("sample"))
        })
}

pub fn safe_path(root: &Path, relative: &Path) -> Result<PathBuf> {
    ensure!(
        !relative.as_os_str().is_empty()
            && relative
                .components()
                .all(|c| matches!(c, Component::Normal(_))),
        "Path must stay inside the task working copy."
    );
    ensure!(
        !relative
            .components()
            .any(|c| matches!(c, Component::Normal(n) if n == ".git")),
        "Git metadata cannot be integrated."
    );
    let mut current = root.to_path_buf();
    let components = relative.components().collect::<Vec<_>>();
    for (index, component) in components.iter().enumerate() {
        current.push(component.as_os_str());
        if index + 1 < components.len() {
            match fs::symlink_metadata(&current) {
                Ok(metadata) => ensure!(
                    metadata.is_dir() && !metadata.is_symlink(),
                    "A parent directory is a symlink or file: {}",
                    current.display()
                ),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
    Ok(current)
}

fn valid_link(relative: &Path, target: &Path) -> bool {
    if target.is_absolute() {
        return false;
    }
    let mut depth = relative.parent().map_or(0, |p| p.components().count());
    for component in target.components() {
        match component {
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            Component::ParentDir if depth > 0 => depth -= 1,
            _ => return false,
        }
    }
    true
}

fn read_file(root: &Path, relative: &Path) -> Result<Option<(SnapshotFile, Vec<u8>)>> {
    let path = safe_path(root, relative)?;
    let metadata = match fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if metadata.is_symlink() {
        let target = fs::read_link(&path)?;
        ensure!(
            valid_link(relative, &target),
            "Symlink escapes the task working copy: {}",
            relative.display()
        );
        let bytes = target
            .to_str()
            .ok_or_else(|| anyhow!("Unsupported non-Unicode symlink."))?
            .as_bytes()
            .to_vec();
        return Ok(Some((
            SnapshotFile {
                hash: hash(&bytes),
                kind: FileKind::Symlink,
                executable: false,
            },
            bytes,
        )));
    }
    ensure!(
        metadata.is_file(),
        "Submodules and special files need a separate assignment: {}",
        relative.display()
    );
    #[cfg(unix)]
    let executable = {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    };
    #[cfg(not(unix))]
    let executable = false;
    let bytes = fs::read(&path)?;
    Ok(Some((
        SnapshotFile {
            hash: hash(&bytes),
            kind: FileKind::Regular,
            executable,
        },
        bytes,
    )))
}

fn put_blob(storage: &Path, bytes: &[u8]) -> Result<String> {
    let digest = hash(bytes);
    let directory = storage.join("blobs");
    fs::create_dir_all(&directory)?;
    let path = directory.join(&digest);
    match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(mut file) => {
            file.write_all(bytes)?;
            file.sync_all()?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => ensure!(
            hash(&fs::read(&path)?) == digest,
            "Snapshot content is corrupted."
        ),
        Err(error) => return Err(error.into()),
    }
    Ok(digest)
}

fn blob(storage: &Path, entry: &SnapshotFile) -> Result<Vec<u8>> {
    ensure!(
        entry.hash.len() == 64 && entry.hash.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid snapshot content hash."
    );
    let bytes = fs::read(storage.join("blobs").join(&entry.hash))?;
    ensure!(
        hash(&bytes) == entry.hash,
        "Snapshot content failed its checksum."
    );
    Ok(bytes)
}

pub fn bounded_text(storage: &Path, entry: Option<&SnapshotFile>) -> Result<Option<String>> {
    let Some(entry) = entry else {
        return Ok(Some(String::new()));
    };
    let bytes = blob(storage, entry)?;
    if bytes.len() > 256_000 || bytes.contains(&0) || entry.kind != FileKind::Regular {
        return Ok(None);
    }
    Ok(String::from_utf8(bytes).ok())
}

fn collect(root: &Path, storage: &Path) -> Result<(BTreeMap<PathBuf, SnapshotFile>, Vec<PathBuf>)> {
    let listed = git(
        root,
        &[
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ],
    )?;
    let mut files = BTreeMap::new();
    let mut omitted = Vec::new();
    for raw in listed.split(|b| *b == 0).filter(|s| !s.is_empty()) {
        let path = PathBuf::from(std::str::from_utf8(raw).context("A task path is not Unicode.")?);
        if excluded(&path) {
            omitted.push(path);
            continue;
        }
        if let Some((entry, bytes)) = read_file(root, &path)? {
            put_blob(storage, &bytes)?;
            files.insert(path, entry);
        }
    }
    omitted.sort();
    omitted.dedup();
    Ok((files, omitted))
}

pub fn capture(source: &Path, storage: &Path) -> Result<WorkspaceSnapshot> {
    let source = source.canonicalize()?;
    let repo = git2::Repository::open(&source)?;
    ensure!(
        repo.state() == git2::RepositoryState::Clean,
        "Finish the repository's merge or rebase before assigning implementation."
    );
    for _ in 0..3 {
        let identity = source_identity(&source)?;
        let (files, excluded) = collect(&source, storage)?;
        let (verified, _) = collect(&source, storage)?;
        if files == verified && identity == source_identity(&source)? {
            return Ok(WorkspaceSnapshot {
                id: Uuid::new_v4(),
                source,
                storage: storage.to_path_buf(),
                head: identity.0,
                branch: identity.1,
                index_hash: identity.2,
                files,
                excluded,
            });
        }
    }
    bail!("The source files kept changing while preparing the assignment. Wait for current edits to settle and retry.")
}

fn write_entry(root: &Path, relative: &Path, storage: &Path, entry: &SnapshotFile) -> Result<()> {
    write_entry_checked(root, relative, storage, entry, || Ok(()))
}

fn write_entry_checked(
    root: &Path,
    relative: &Path,
    storage: &Path,
    entry: &SnapshotFile,
    before_replace: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let destination = safe_path(root, relative)?;
    let parent = destination.parent().context("File has no parent.")?;
    fs::create_dir_all(parent)?;
    let bytes = blob(storage, entry)?;
    let temporary = parent.join(format!(".choro-integration-{}", Uuid::new_v4()));
    if entry.kind == FileKind::Symlink {
        let target = PathBuf::from(std::str::from_utf8(&bytes)?);
        ensure!(
            valid_link(relative, &target),
            "Symlink escapes the working copy."
        );
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, &temporary)?;
        #[cfg(not(unix))]
        bail!("Symlink integration is unsupported on this platform.");
    } else {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(if entry.executable {
                0o755
            } else {
                0o644
            }))?;
        }
        file.sync_all()?;
    }
    // Preparing a large postimage can take time. Recheck the durable gate and
    // source after that I/O, immediately before replacing the destination.
    before_replace()?;
    safe_path(root, relative)?;
    fs::rename(&temporary, &destination)?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

pub fn materialize(snapshot: &WorkspaceSnapshot, destination: &Path) -> Result<()> {
    ensure!(
        !destination.exists(),
        "The task working copy already exists; recover it instead of replacing it."
    );
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    if snapshot.head.is_some() {
        let output = Command::new("git")
            .args(["clone", "--no-hardlinks", "--no-checkout", "--local", "--"])
            .arg(&snapshot.source)
            .arg(destination)
            .output()?;
        ensure!(
            output.status.success(),
            "Could not copy repository history: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    } else {
        git2::Repository::init(destination)?;
    }
    let repo = git2::Repository::open(destination)?;
    if repo.find_remote("origin").is_ok() {
        repo.remote_delete("origin")?;
    }
    for (path, entry) in &snapshot.files {
        write_entry(destination, path, &snapshot.storage, entry)?;
    }
    let mut index = repo.index()?;
    index.clear()?;
    for path in snapshot.files.keys() {
        index.add_path(path)?;
    }
    index.write()?;
    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;
    let parent = snapshot
        .head
        .as_ref()
        .and_then(|h| git2::Oid::from_str(h).ok())
        .and_then(|id| repo.find_commit(id).ok());
    let parents = parent.as_ref().into_iter().collect::<Vec<_>>();
    let signature = git2::Signature::now("Choro snapshot", "snapshot@localhost")?;
    repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        "Choro task starting snapshot (private)",
        &tree,
        &parents,
    )?;
    Ok(())
}

/// Restore exported files into an app-owned copy whose Git history was
/// separately restored. No runtime starts and no source repository is touched.
pub fn restore_archived_files(snapshot: &WorkspaceSnapshot, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)?;
    for (path, entry) in &snapshot.files {
        write_entry(destination, path, &snapshot.storage, entry)?;
    }
    Ok(())
}

fn merge_regular(
    storage: &Path,
    _directory: &Path,
    base: &SnapshotFile,
    expert: &SnapshotFile,
    parent: &SnapshotFile,
) -> Result<Option<Vec<u8>>> {
    if base.kind != FileKind::Regular
        || expert.kind != FileKind::Regular
        || parent.kind != FileKind::Regular
    {
        return Ok(None);
    }
    let base_bytes = blob(storage, base)?;
    let expert_bytes = blob(storage, expert)?;
    let parent_bytes = blob(storage, parent)?;
    if [&base_bytes, &expert_bytes, &parent_bytes]
        .iter()
        .any(|b| b.contains(&0))
    {
        return Ok(None);
    }
    let scratch = storage.join(format!("merge-{}", Uuid::new_v4()));
    fs::create_dir_all(&scratch)?;
    fs::write(scratch.join("base"), base_bytes)?;
    fs::write(scratch.join("expert"), expert_bytes)?;
    fs::write(scratch.join("parent"), parent_bytes)?;
    let output = Command::new("git")
        .args(["merge-file", "-p", "--diff3", "--"])
        .arg(scratch.join("parent"))
        .arg(scratch.join("base"))
        .arg(scratch.join("expert"))
        .output()?;
    if output.status.success() {
        Ok(Some(output.stdout))
    } else {
        Ok(None)
    }
}

pub fn prepare_integration(
    base: &WorkspaceSnapshot,
    expert_root: &Path,
) -> Result<IntegrationOperation> {
    let expert = capture(expert_root, &base.storage)?;
    let parent = capture(&base.source, &base.storage)?;
    ensure!(parent.head==base.head && parent.branch==base.branch,"The source branch or HEAD changed since assignment. Return to the recorded source before integrating.");
    let id = Uuid::new_v4();
    let resolution_dir = base.storage.join(format!("integration-{id}"));
    materialize(&parent, &resolution_dir)?;
    let mut operation = IntegrationOperation {
        id,
        source: base.source.clone(),
        storage: base.storage.clone(),
        head: parent.head,
        branch: parent.branch,
        index_hash: parent.index_hash,
        changes: Vec::new(),
        conflicts: Vec::new(),
        resolution_dir,
        applied_paths: Vec::new(),
        status: IntegrationStatus::Prepared,
        archive_snapshot: None,
    };
    let paths = base
        .files
        .keys()
        .chain(expert.files.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    for path in paths {
        let b = base.files.get(&path);
        let e = expert.files.get(&path);
        let p = parent.files.get(&path);
        if b == e || p == e {
            continue;
        }
        let mut after = e.cloned();
        if p != b {
            let merged = match (b, e, p) {
                (Some(b), Some(e), Some(p)) => {
                    merge_regular(&base.storage, &operation.resolution_dir, b, e, p)?
                }
                _ => None,
            };
            if let Some(bytes) = merged {
                let mut entry = e.expect("regular merge").clone();
                entry.hash = put_blob(&base.storage, &bytes)?;
                let original_mode = b.expect("regular merge").executable;
                let parent_mode = p.expect("regular merge").executable;
                if entry.executable == original_mode {
                    entry.executable = parent_mode;
                }
                after = Some(entry);
            } else {
                operation.conflicts.push(path.clone());
            }
        }
        // Conflicted files remain the parent's version. The lead gets the
        // base/expert manifests and edits the resolution copy explicitly.
        if !operation.conflicts.contains(&path) {
            if let Some(entry) = &after {
                write_entry(&operation.resolution_dir, &path, &base.storage, entry)?;
            }
        }
        operation.changes.push(IntegrationChange {
            path,
            before: p.cloned(),
            after,
        });
    }
    if !operation.conflicts.is_empty() {
        operation.status = IntegrationStatus::Conflict;
    }
    operation.save_journal()?;
    Ok(operation)
}

impl IntegrationOperation {
    pub fn matches_current_source(&self) -> Result<bool> {
        if source_identity(&self.source)?
            != (
                self.head.clone(),
                self.branch.clone(),
                self.index_hash.clone(),
            )
        {
            return Ok(false);
        }
        for c in &self.changes {
            let current = read_file(&self.source, &c.path)?.map(|(entry, _)| entry);
            if current != c.before && current != c.after {
                return Ok(false);
            }
        }
        Ok(true)
    }
    /// Recompute only unapplied contributions, keeping the previous journal as
    /// evidence. Already applied files may have newer user edits; never replay
    /// those contributions over them after a crash or a conflict.
    pub fn reprepare_remaining(
        &self,
        baseline: &WorkspaceSnapshot,
        expert_root: &Path,
    ) -> Result<Self> {
        let completed = capture(expert_root, &baseline.storage)?;
        let mut remaining = baseline.clone();
        for c in &self.changes {
            let current = read_file(&self.source, &c.path)?.map(|(entry, _)| entry);
            if self.applied_paths.contains(&c.path) || current == c.after {
                if let Some(entry) = completed.files.get(&c.path) {
                    remaining.files.insert(c.path.clone(), entry.clone());
                } else {
                    remaining.files.remove(&c.path);
                }
            }
        }
        prepare_integration(&remaining, expert_root)
    }
    pub fn journal_path(&self) -> PathBuf {
        self.storage.join(format!("integration-{}.json", self.id))
    }
    pub fn save_journal(&self) -> Result<()> {
        fs::create_dir_all(&self.storage)?;
        let destination = self.journal_path();
        let temporary = self.storage.join(format!("journal-{}.tmp", Uuid::new_v4()));
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        file.write_all(&serde_json::to_vec(self)?)?;
        file.sync_all()?;
        fs::rename(temporary, destination)?;
        File::open(&self.storage)?.sync_all()?;
        Ok(())
    }
    pub fn accept_resolutions(&mut self) -> Result<()> {
        ensure!(
            self.status == IntegrationStatus::Conflict,
            "This integration has no pending conflicts."
        );
        for change in &mut self.changes {
            if !self.conflicts.contains(&change.path) {
                continue;
            }
            change.after = match read_file(&self.resolution_dir, &change.path)? {
                Some((entry, bytes)) => {
                    put_blob(&self.storage, &bytes)?;
                    Some(entry)
                }
                None => None,
            };
        }
        self.conflicts.clear();
        self.status = IntegrationStatus::Prepared;
        self.save_journal()
    }
    pub fn needs_deletion_confirmation(&self) -> bool {
        self.changes
            .iter()
            .any(|c| c.after.is_none() && c.before.is_some())
    }
    pub fn apply(&mut self, deletions_confirmed: bool) -> Result<()> {
        self.apply_guarded(deletions_confirmed, || Ok(()))
    }
    /// The durable run gate is checked before and during application. A pause
    /// can leave an honest partial journal, never a falsely atomic receipt.
    pub fn apply_guarded(
        &mut self,
        deletions_confirmed: bool,
        mut gate: impl FnMut() -> Result<()>,
    ) -> Result<()> {
        gate()?;
        ensure!(
            matches!(
                self.status,
                IntegrationStatus::Prepared
                    | IntegrationStatus::Applying
                    | IntegrationStatus::Applied
            ),
            "Resolve integration conflicts before applying."
        );
        ensure!(!self.needs_deletion_confirmation() || deletions_confirmed, "This integration removes files. Confirm the listed deletions in Choro before applying.");
        let identity = source_identity(&self.source)?;
        ensure!(
            identity
                == (
                    self.head.clone(),
                    self.branch.clone(),
                    self.index_hash.clone()
                ),
            "The source branch or index changed; prepare integration again."
        );
        // Validate the entire set before the first write. Recheck each path
        // immediately before replacing it as well; a journal handles crashes.
        for change in &self.changes {
            let current = read_file(&self.source, &change.path)?.map(|(entry, _)| entry);
            ensure!(
                current == change.before || current == change.after,
                "{} changed after integration was prepared. Recompute the integration.",
                change.path.display()
            );
        }
        self.status = IntegrationStatus::Applying;
        self.save_journal()?;
        for change in self.changes.clone() {
            gate()?;
            ensure!(source_identity(&self.source)? == identity,
                "The source branch or index changed during integration. Reconcile the journal before continuing.");
            let current = read_file(&self.source, &change.path)?.map(|(entry, _)| entry);
            if current != change.after {
                if current != change.before {
                    self.status = IntegrationStatus::NeedsAttention;
                    self.save_journal()?;
                    bail!("{} changed during integration. Recorded changes and preimages remain available.", change.path.display());
                }
                match &change.after {
                    Some(entry) => write_entry_checked(
                        &self.source,
                        &change.path,
                        &self.storage,
                        entry,
                        || {
                            gate()?;
                            ensure!(source_identity(&self.source)? == identity,
                            "The source branch or index changed while preparing a file replacement.");
                            let latest =
                                read_file(&self.source, &change.path)?.map(|(entry, _)| entry);
                            ensure!(latest == change.before,
                            "{} changed while preparing its replacement. Recompute integration; the newer file is preserved.", change.path.display());
                            Ok(())
                        },
                    )?,
                    None => {
                        let path = safe_path(&self.source, &change.path)?;
                        fs::remove_file(&path)?;
                        File::open(path.parent().context("File has no parent")?)?.sync_all()?;
                    }
                }
            }
            if !self.applied_paths.contains(&change.path) {
                self.applied_paths.push(change.path);
            }
            self.save_journal()?;
        }
        self.status = IntegrationStatus::Applied;
        self.save_journal()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn repository(root: &Path) {
        let repo = git2::Repository::init(root).unwrap();
        fs::write(root.join("a.txt"), "one\ntwo\nthree\nfour\nfive\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("a.txt")).unwrap();
        index.write().unwrap();
        let id = index.write_tree().unwrap();
        let tree = repo.find_tree(id).unwrap();
        let sig = git2::Signature::now("Test", "test@localhost").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "base", &tree, &[])
            .unwrap();
    }
    #[test]
    fn dirty_snapshot_and_integration_preserve_index_and_parent_edits() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        repository(&source);
        fs::write(source.join("new.txt"), "uncommitted\n").unwrap();
        fs::write(source.join("a.txt"), "dirty\ntwo\nthree\nfour\nfive\n").unwrap();
        let identity = source_identity(&source).unwrap();
        let base = capture(&source, &dir.path().join("store")).unwrap();
        let child = dir.path().join("child");
        materialize(&base, &child).unwrap();
        assert_eq!(fs::read(child.join("new.txt")).unwrap(), b"uncommitted\n");
        fs::write(child.join("a.txt"), "expert\ntwo\nthree\nfour\nfive\n").unwrap();
        fs::write(source.join("a.txt"), "dirty\ntwo\nthree\nfour\nparent\n").unwrap();
        let mut op = prepare_integration(&base, &child).unwrap();
        assert_eq!(op.status, IntegrationStatus::Prepared);
        op.apply(false).unwrap();
        op.apply(false).unwrap();
        assert_eq!(
            fs::read_to_string(source.join("a.txt")).unwrap(),
            "expert\ntwo\nthree\nfour\nparent\n"
        );
        assert_eq!(source_identity(&source).unwrap(), identity);
        assert!(child.is_dir());
    }
    #[test]
    fn source_edits_during_postimage_preparation_are_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        repository(&source);
        let base = capture(&source, &dir.path().join("store")).unwrap();
        let child = dir.path().join("child");
        materialize(&base, &child).unwrap();
        fs::write(child.join("a.txt"), "bandmate contribution").unwrap();
        let mut op = prepare_integration(&base, &child).unwrap();
        let mut gates = 0;
        let error = op
            .apply_guarded(false, || {
                gates += 1;
                if gates == 3 {
                    fs::write(source.join("a.txt"), "newer user edit")?;
                }
                Ok(())
            })
            .unwrap_err();
        assert!(error.to_string().contains("newer file is preserved"));
        assert_eq!(
            fs::read_to_string(source.join("a.txt")).unwrap(),
            "newer user edit"
        );
        let journal: IntegrationOperation =
            serde_json::from_slice(&fs::read(op.journal_path()).unwrap()).unwrap();
        assert_eq!(journal.status, IntegrationStatus::Applying);
        assert!(journal.applied_paths.is_empty());
        assert!(!journal.matches_current_source().unwrap());
    }

    #[test]
    fn conflicts_and_late_edits_do_not_overwrite_source() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        repository(&source);
        let base = capture(&source, &dir.path().join("store")).unwrap();
        let child = dir.path().join("child");
        materialize(&base, &child).unwrap();
        fs::write(child.join("a.txt"), "expert\n").unwrap();
        fs::write(source.join("a.txt"), "parent\n").unwrap();
        let mut op = prepare_integration(&base, &child).unwrap();
        assert_eq!(op.status, IntegrationStatus::Conflict);
        assert!(op.apply(false).is_err());
        fs::write(op.resolution_dir.join("a.txt"), "combined\n").unwrap();
        op.accept_resolutions().unwrap();
        fs::write(source.join("a.txt"), "later\n").unwrap();
        assert!(op.apply(false).is_err());
        assert_eq!(fs::read_to_string(source.join("a.txt")).unwrap(), "later\n");
    }
    #[test]
    fn unborn_repository_and_path_escape() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        git2::Repository::init(&source).unwrap();
        fs::write(source.join("new.txt"), [0, 1, 2]).unwrap();
        let base = capture(&source, &dir.path().join("store")).unwrap();
        materialize(&base, &dir.path().join("child")).unwrap();
        assert!(safe_path(&source, Path::new("../outside")).is_err());
        assert!(safe_path(&source, Path::new(".git/config")).is_err());
        assert_eq!(
            fs::read(dir.path().join("child/new.txt")).unwrap(),
            vec![0, 1, 2]
        );
    }
    #[test]
    fn deletion_requires_confirmation_and_preserves_the_index() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        repository(&source);
        let base = capture(&source, &dir.path().join("store")).unwrap();
        let child = dir.path().join("child");
        materialize(&base, &child).unwrap();
        let identity = source_identity(&source).unwrap();
        fs::remove_file(child.join("a.txt")).unwrap();
        let mut op = prepare_integration(&base, &child).unwrap();
        assert!(op.needs_deletion_confirmation());
        assert!(op.apply(false).is_err());
        assert!(source.join("a.txt").is_file());
        op.apply(true).unwrap();
        assert!(!source.join("a.txt").exists());
        assert_eq!(identity, source_identity(&source).unwrap());
        assert!(child.exists());
    }
    #[test]
    fn interrupted_multi_file_apply_reconciles_its_journal() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        repository(&source);
        fs::write(source.join("b.txt"), "before").unwrap();
        let base = capture(&source, &dir.path().join("store")).unwrap();
        let child = dir.path().join("child");
        materialize(&base, &child).unwrap();
        fs::write(child.join("a.txt"), "after A").unwrap();
        fs::write(child.join("b.txt"), "after B").unwrap();
        let mut op = prepare_integration(&base, &child).unwrap();
        let mut gates = 0;
        assert!(op
            .apply_guarded(false, || {
                gates += 1;
                ensure!(gates < 4, "Simulated durable Stop");
                Ok(())
            })
            .is_err());
        let mut recovered: IntegrationOperation =
            serde_json::from_slice(&fs::read(op.journal_path()).unwrap()).unwrap();
        assert_eq!(recovered.status, IntegrationStatus::Applying);
        assert_eq!(recovered.applied_paths.len(), 1);
        assert_eq!(fs::read_to_string(source.join("b.txt")).unwrap(), "before");
        recovered.apply(false).unwrap();
        recovered.apply(false).unwrap();
        assert_eq!(fs::read_to_string(source.join("b.txt")).unwrap(), "after B");
    }
    #[cfg(unix)]
    #[test]
    fn executable_unicode_and_symlink_metadata_survive_isolation_and_integration() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        repository(&source);
        fs::write(source.join("שלום.sh"), "#!/bin/sh\ntrue\n").unwrap();
        symlink("a.txt", source.join("link")).unwrap();
        let base = capture(&source, &dir.path().join("store")).unwrap();
        let child = dir.path().join("child");
        materialize(&base, &child).unwrap();
        assert_eq!(
            fs::read_link(child.join("link")).unwrap(),
            Path::new("a.txt")
        );
        fs::set_permissions(child.join("שלום.sh"), fs::Permissions::from_mode(0o755)).unwrap();
        let mut op = prepare_integration(&base, &child).unwrap();
        op.apply(false).unwrap();
        assert_ne!(
            fs::metadata(source.join("שלום.sh"))
                .unwrap()
                .permissions()
                .mode()
                & 0o111,
            0
        );
        symlink("../outside", source.join("escape")).unwrap();
        assert!(capture(&source, &base.storage).is_err());
    }
    #[test]
    fn each_followup_baseline_only_contributes_its_new_revision() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        repository(&source);
        for revision in 1..=2 {
            let base = capture(&source, &dir.path().join("store")).unwrap();
            let child = dir.path().join(format!("child-{revision}"));
            materialize(&base, &child).unwrap();
            let mut content = fs::read_to_string(child.join("a.txt")).unwrap();
            content.push_str(&format!("revision {revision}\n"));
            fs::write(child.join("a.txt"), content).unwrap();
            let mut op = prepare_integration(&base, &child).unwrap();
            op.apply(false).unwrap();
        }
        let content = fs::read_to_string(source.join("a.txt")).unwrap();
        assert_eq!(content.matches("revision 1").count(), 1);
        assert_eq!(content.matches("revision 2").count(), 1);
    }
}
