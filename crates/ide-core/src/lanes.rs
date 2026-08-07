//! Solo agent lanes.
//!
//! A lane is a git worktree at a stable, per-agent path where a Solo agent
//! works without touching the project's main working directory. Lanes are
//! disposable: every exit (Rejoin / Ship PR / Discard) tears the folder down
//! and only the branch survives; recreation lands at the same path so
//! provider session resume (which is cwd-sensitive for Claude) keeps working.
//!
//! Every lane receives a read-only snapshot of canonical Choro Docs because
//! those files are normally gitignored. Setup is otherwise profile-aware:
//! `Full` also copies env files and clones dependency folders via APFS
//! copy-on-write (`cp -c`), falling back to a plain copy off APFS. The first
//! agent turn waits for every requested setup step.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::agents::LaneProfile;
use crate::branding::DOCS_DIR_NAME;
use crate::config::AppConfig;
use crate::git::remote::{self, RemoteOutput};
use crate::git::{read_head, read_snapshot};
use crate::project::ProjectId;
use crate::services::detect_project_services;

/// One step of lane preparation, in execution order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneStep {
    Worktree,
    ChoroDocs,
    EnvFiles,
    Dependencies,
}

impl LaneStep {
    pub fn label(self) -> &'static str {
        match self {
            Self::Worktree => "Branch & folder",
            Self::ChoroDocs => "Choro Docs",
            Self::EnvFiles => "Env files",
            Self::Dependencies => "Dependencies",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneStepResult {
    pub step: LaneStep,
    pub ok: bool,
    pub detail: String,
}

impl LaneStepResult {
    fn ok(step: LaneStep, detail: impl Into<String>) -> Self {
        Self {
            step,
            ok: true,
            detail: detail.into(),
        }
    }

    fn failed(step: LaneStep, detail: impl Into<String>) -> Self {
        Self {
            step,
            ok: false,
            detail: detail.into(),
        }
    }
}

/// Root directory for all lanes, inside Choro's data dir — never inside the
/// user's project.
pub fn lane_root() -> PathBuf {
    AppConfig::config_path()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("lanes")
}

/// The stable lane location for one agent. Deterministic on purpose: Claude
/// stores session transcripts under a slug of the working directory, so a
/// recreated lane must land at exactly the same path for resume to work.
pub fn lane_path_for(project_id: ProjectId, agent_id: Uuid) -> PathBuf {
    lane_root()
        .join(project_id.0.to_string())
        .join(agent_id.to_string())
}

/// Default setup profile for a project. iOS-marked projects default to
/// `CodeOnly`: their builds (Pods, DerivedData) are too expensive to duplicate
/// per lane, so the lane materializes instantly and builds stay explicit.
pub fn default_profile(project_root: &Path) -> LaneProfile {
    if project_root.join("ios").is_dir() || project_root.join("Podfile").is_file() {
        return LaneProfile::CodeOnly;
    }
    let has_xcode_project = fs::read_dir(project_root)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.ends_with(".xcodeproj") || name.ends_with(".xcworkspace")
        });
    if has_xcode_project {
        LaneProfile::CodeOnly
    } else {
        LaneProfile::Full
    }
}

/// Whether the lane has uncommitted (non-ignored) changes. Gitignored files —
/// the copied env files and cloned dependencies — do not count.
pub fn lane_is_dirty(lane_path: &Path) -> Result<bool> {
    Ok(!read_snapshot(lane_path)?.entries.is_empty())
}

/// A unique Solo branch name (`solo/<slug>-<agent>`) derived from the title and
/// the already-created agent id. The id is the reservation: two launches can
/// choose names before either asynchronous worktree exists without colliding.
pub fn solo_branch_name(seed: &str, agent_id: Uuid) -> String {
    let mut slug = String::new();
    let mut pending_dash = false;
    for ch in seed.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            slug.push(ch.to_ascii_lowercase());
            pending_dash = false;
        } else {
            pending_dash = true;
        }
        if slug.len() >= 42 {
            break;
        }
    }
    if slug.is_empty() {
        slug = "work".to_string();
    }
    let suffix = agent_id.simple();
    format!("solo/{slug}-{suffix}")
}

/// Whether a local branch exists — used to choose between creating a fresh
/// Solo branch and re-checking-out one that survived a lane teardown.
pub fn branch_exists(project_root: &Path, name: &str) -> bool {
    git2::Repository::open(project_root)
        .ok()
        .is_some_and(|repo| repo.find_branch(name, git2::BranchType::Local).is_ok())
}

/// The worktree step alone: branch + folder. The UI can report this separately
/// from the idempotent env/dependency preparation that follows.
/// `base: Some(branch)` creates the Solo branch fresh from it; `base: None`
/// re-checks-out an existing Solo branch (lane recreation).
pub fn materialize_worktree(
    project_root: &Path,
    lane_path: &Path,
    branch: &str,
    base: Option<&str>,
) -> LaneStepResult {
    // Already materialized as this branch (e.g. app restarted mid-setup):
    // the worktree step is idempotent.
    let already = read_head(lane_path)
        .ok()
        .and_then(|head| head.branch)
        .is_some_and(|current| current == branch);
    if already {
        return LaneStepResult::ok(LaneStep::Worktree, "already prepared");
    }
    // Drop stale bookkeeping from a previously deleted lane dir first.
    let _ = remote::worktree_prune(project_root);
    if lane_path.exists() {
        // Only an empty leftover directory may be swept aside; anything
        // else is unknown content we refuse to clobber.
        let is_empty = fs::read_dir(lane_path)
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(false);
        if is_empty {
            let _ = fs::remove_dir(lane_path);
        } else {
            return LaneStepResult::failed(
                LaneStep::Worktree,
                "the lane folder already exists and isn't empty — remove it and retry",
            );
        }
    }
    if let Some(parent) = lane_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let added = match base {
        Some(base) => remote::worktree_add(project_root, lane_path, branch, base),
        None => remote::worktree_add_existing(project_root, lane_path, branch),
    };
    match added {
        Ok(output) if output.success => {
            // Git's common info/exclude is shared by the main tree and every
            // linked worktree. Install the local dependency guard as soon as
            // the lane exists so an agent-run package install stays invisible
            // to status, Stage All, Ship, and Rejoin.
            match crate::git::ensure_local_dependency_excludes(lane_path) {
                Ok(_) => LaneStepResult::ok(LaneStep::Worktree, "ready"),
                Err(error) => LaneStepResult::ok(
                    LaneStep::Worktree,
                    format!("ready; dependency safety unavailable: {error:#}"),
                ),
            }
        }
        Ok(output) => LaneStepResult::failed(LaneStep::Worktree, output.message()),
        Err(error) => LaneStepResult::failed(LaneStep::Worktree, format!("{error:#}")),
    }
}

/// Env + dependency preparation, run after the worktree exists. A failure here
/// is non-fatal — the lane still works, just without the copied extras.
pub fn prepare_extras(
    project_root: &Path,
    lane_path: &Path,
    profile: LaneProfile,
) -> Vec<LaneStepResult> {
    if profile == LaneProfile::CodeOnly {
        return vec![
            LaneStepResult::ok(LaneStep::EnvFiles, "skipped"),
            LaneStepResult::ok(LaneStep::Dependencies, "skipped"),
        ];
    }
    vec![
        copy_env_files(project_root, lane_path),
        clone_dependencies(project_root, lane_path),
    ]
}

const SOLO_DOCS_MANIFEST: &str = ".solo-snapshot.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SoloDocsSnapshotManifest {
    version: u32,
    revision: String,
    files: Vec<PathBuf>,
}

/// Refresh the read-only Choro Docs context inside a Solo lane.
///
/// Choro Docs are normally ignored and therefore absent from a Git worktree.
/// The canonical project directory remains the only editable copy; the lane
/// receives generated snapshots at the same relative paths used by prompts.
/// Only files recorded in Choro's own manifest are ever removed on refresh.
pub fn refresh_choro_docs_snapshot(
    canonical_project_root: &Path,
    lane_path: &Path,
) -> LaneStepResult {
    match refresh_choro_docs_snapshot_inner(canonical_project_root, lane_path) {
        Ok(detail) => LaneStepResult::ok(LaneStep::ChoroDocs, detail),
        Err(error) => LaneStepResult::failed(LaneStep::ChoroDocs, format!("{error:#}")),
    }
}

fn refresh_choro_docs_snapshot_inner(
    canonical_project_root: &Path,
    lane_path: &Path,
) -> Result<String> {
    if lane_tracks_choro_docs(lane_path)? {
        return Ok("tracked by Git".to_string());
    }
    crate::git::ensure_local_choro_docs_exclude(lane_path)
        .context("failed to protect the Solo document snapshot from Git")?;

    let source_root = canonical_project_root.join(DOCS_DIR_NAME);
    let destination_root = lane_path.join(DOCS_DIR_NAME);
    let manifest_path = destination_root.join(SOLO_DOCS_MANIFEST);
    let previous = read_solo_docs_manifest(&manifest_path)?;
    if destination_root.exists() && previous.is_none() {
        let is_empty = fs::read_dir(&destination_root)
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(false);
        anyhow::ensure!(
            is_empty,
            "{} already contains files not managed by Choro; the snapshot was not replaced",
            destination_root.display()
        );
    }

    let mut files = Vec::new();
    if source_root.is_dir() {
        collect_choro_docs_snapshot_files(&source_root, &source_root, false, &mut files)?;
    }
    files.sort();
    let revision = snapshot_revision(&source_root, &files)?;

    fs::create_dir_all(&destination_root)
        .with_context(|| format!("failed to create {}", destination_root.display()))?;
    set_directory_writable(&destination_root)?;
    if let Some(previous) = previous.as_ref() {
        set_snapshot_directories_writable(&destination_root, &previous.files)?;
    }

    let current = files
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    if let Some(previous) = previous {
        for relative in previous.files {
            if current.contains(&relative) || !safe_snapshot_relative_path(&relative) {
                continue;
            }
            let stale = destination_root.join(&relative);
            if let Some(parent) = stale.parent() {
                set_directory_writable(parent)?;
            }
            if stale.is_file() {
                fs::remove_file(&stale).with_context(|| {
                    format!("failed to replace stale snapshot {}", stale.display())
                })?;
            }
        }
    }

    for relative in &files {
        let source = source_root.join(relative);
        let destination = destination_root.join(relative);
        let parent = destination
            .parent()
            .context("snapshot destination has no parent")?;
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
        set_directory_writable(parent)?;
        if destination.is_file() {
            set_file_writable(&destination)?;
        }
        fs::copy(&source, &destination).with_context(|| {
            format!(
                "failed to snapshot {} into {}",
                source.display(),
                destination.display()
            )
        })?;
        set_file_read_only(&destination)?;
    }

    let manifest = SoloDocsSnapshotManifest {
        version: 1,
        revision: revision.clone(),
        files: files.clone(),
    };
    if manifest_path.is_file() {
        set_file_writable(&manifest_path)?;
    }
    fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest)?)
        .with_context(|| format!("failed to write {}", manifest_path.display()))?;
    set_file_read_only(&manifest_path)?;
    set_snapshot_directories_read_only(&destination_root, &files)?;

    let short_revision = revision.get(..8).unwrap_or(&revision);
    Ok(format!("{} files · {short_revision}", files.len()))
}

fn lane_tracks_choro_docs(lane_path: &Path) -> Result<bool> {
    let repository =
        git2::Repository::open(lane_path).context("Solo lane is not a Git worktree")?;
    let index = repository
        .index()
        .context("failed to read Solo Git index")?;
    Ok(index.iter().any(|entry| {
        std::str::from_utf8(&entry.path)
            .ok()
            .is_some_and(|path| Path::new(path).starts_with(DOCS_DIR_NAME))
    }))
}

fn read_solo_docs_manifest(path: &Path) -> Result<Option<SoloDocsSnapshotManifest>> {
    match fs::read(path) {
        Ok(bytes) => {
            let manifest: SoloDocsSnapshotManifest = serde_json::from_slice(&bytes)
                .with_context(|| format!("failed to read {}", path.display()))?;
            anyhow::ensure!(
                manifest.version == 1,
                "unsupported Solo docs snapshot version"
            );
            Ok(Some(manifest))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("failed to read {}", path.display())),
    }
}

fn collect_choro_docs_snapshot_files(
    root: &Path,
    directory: &Path,
    inside_assets: bool,
    files: &mut Vec<PathBuf>,
) -> Result<()> {
    for entry in fs::read_dir(directory)
        .with_context(|| format!("failed to read {}", directory.display()))?
    {
        let entry = entry.context("failed to inspect Choro Docs entry")?;
        let file_type = entry
            .file_type()
            .context("failed to inspect Choro Docs entry type")?;
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        if file_type.is_dir() {
            let is_assets =
                path.extension().and_then(|extension| extension.to_str()) == Some("assets");
            collect_choro_docs_snapshot_files(root, &path, inside_assets || is_assets, files)?;
        } else if file_type.is_file()
            && (inside_assets
                || path.extension().and_then(|extension| extension.to_str()) == Some("choro"))
        {
            files.push(
                path.strip_prefix(root)
                    .context("snapshot file escaped Choro Docs root")?
                    .to_path_buf(),
            );
        }
    }
    Ok(())
}

fn snapshot_revision(root: &Path, files: &[PathBuf]) -> Result<String> {
    let mut digest = Sha256::new();
    for relative in files {
        digest.update(relative.to_string_lossy().as_bytes());
        digest.update([0]);
        digest.update(
            fs::read(root.join(relative))
                .with_context(|| format!("failed to hash {}", relative.display()))?,
        );
        digest.update([0]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn safe_snapshot_relative_path(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
}

fn snapshot_directories(root: &Path, files: &[PathBuf]) -> Vec<PathBuf> {
    let mut directories = files
        .iter()
        .filter_map(|relative| relative.parent())
        .map(|relative| root.join(relative))
        .collect::<Vec<_>>();
    directories.push(root.to_path_buf());
    directories.sort();
    directories.dedup();
    directories
}

fn set_snapshot_directories_writable(root: &Path, files: &[PathBuf]) -> Result<()> {
    for directory in snapshot_directories(root, files) {
        set_directory_writable(&directory)?;
    }
    Ok(())
}

fn set_snapshot_directories_read_only(root: &Path, files: &[PathBuf]) -> Result<()> {
    let mut directories = snapshot_directories(root, files);
    directories.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    for directory in directories {
        set_directory_read_only(&directory)?;
    }
    Ok(())
}

fn unlock_choro_docs_snapshot(lane_path: &Path) -> Result<()> {
    let root = lane_path.join(DOCS_DIR_NAME);
    let manifest = read_solo_docs_manifest(&root.join(SOLO_DOCS_MANIFEST))?;
    if let Some(manifest) = manifest {
        set_snapshot_directories_writable(&root, &manifest.files)?;
    }
    Ok(())
}

#[cfg(unix)]
fn set_directory_writable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    if path.is_dir() {
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
            .with_context(|| format!("failed to unlock {}", path.display()))?;
    }
    Ok(())
}

#[cfg(unix)]
fn set_directory_read_only(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    if path.is_dir() {
        fs::set_permissions(path, fs::Permissions::from_mode(0o555))
            .with_context(|| format!("failed to protect {}", path.display()))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn set_directory_read_only(path: &Path) -> Result<()> {
    if path.is_dir() {
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_readonly(true);
        fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn set_directory_writable(path: &Path) -> Result<()> {
    if path.is_dir() {
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_readonly(false);
        fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

#[cfg(unix)]
fn set_file_read_only(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o444))
        .with_context(|| format!("failed to protect {}", path.display()))?;
    Ok(())
}

#[cfg(unix)]
fn set_file_writable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o644))
        .with_context(|| format!("failed to unlock {}", path.display()))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_file_writable(path: &Path) -> Result<()> {
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_readonly(false);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_file_read_only(path: &Path) -> Result<()> {
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_readonly(true);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

/// Materialize a lane end to end: worktree, Choro Docs, then env/deps per profile.
/// Returns per-step results for the "Preparing lane" UI; stops after a failed
/// worktree step (nothing to prepare into).
pub fn create_lane(
    project_root: &Path,
    lane_path: &Path,
    branch: &str,
    base: Option<&str>,
    profile: LaneProfile,
) -> Vec<LaneStepResult> {
    let worktree = materialize_worktree(project_root, lane_path, branch, base);
    if !worktree.ok {
        return vec![worktree];
    }
    let mut results = vec![worktree];
    results.push(refresh_choro_docs_snapshot(project_root, lane_path));
    results.extend(prepare_extras(project_root, lane_path, profile));
    results
}

/// Outcome of a Rejoin attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RejoinOutcome {
    /// Merged into the main tree's current branch; the lane is torn down.
    Merged,
    /// The merge landed, but the worktree could not be removed. The caller
    /// must keep tracking the lane so the user can retry cleanup safely.
    MergedLaneRetained(String),
    /// The merge could not complete (conflicts, or uncommitted overlap in the
    /// main tree). It was aborted — main tree restored and the lane remains
    /// available. Dirty lane content may now be preserved in a lane commit.
    Conflict(String),
}

/// Rejoin: bring a Solo's work back into a branch of the main tree.
///
/// 1. Uncommitted lane work is committed first (stage-all + commit), so
///    nothing is lost and the merge sees the whole Solo.
/// 2. With a `target` that isn't checked out, the main tree switches to it
///    first (git's safe checkout — it refuses to clobber dirty files).
/// 3. `git merge --no-ff` runs in the MAIN tree. Git itself refuses merges
///    that would clobber dirty files there — we don't pre-block.
/// 4. Success tears the lane down (branch survives). Failure aborts the merge
///    and reports git's own message verbatim.
pub fn rejoin(
    project_root: &Path,
    lane_path: &Path,
    branch: &str,
    commit_title: &str,
    target: Option<&str>,
) -> Result<RejoinOutcome> {
    crate::git::ensure_local_dependency_excludes(lane_path)
        .context("couldn't install the local dependency safety rules before Rejoin")?;
    if lane_is_dirty(lane_path)? {
        let staged = remote::stage_all(lane_path)?;
        if !staged.success {
            return Ok(RejoinOutcome::Conflict(staged.message()));
        }
        let message = if commit_title.trim().is_empty() {
            "Solo work".to_string()
        } else {
            format!("Solo work: {}", commit_title.trim())
        };
        crate::git::write::commit(lane_path, &message)?;
    }

    if let Some(target) = target {
        let current = read_head(project_root).ok().and_then(|head| head.branch);
        if current.as_deref() != Some(target) {
            if let Err(error) = crate::git::write::checkout_branch(project_root, target) {
                return Ok(RejoinOutcome::Conflict(format!(
                    "Couldn't switch to {target}: {error:#}"
                )));
            }
        }
    }

    let merged = remote::merge(project_root, branch)?;
    if !merged.success {
        // A refused merge can leave the repository clean, in which case there
        // is nothing to abort. If Git did enter a merge state, cleanup is not
        // best-effort: surfacing an abort failure prevents the UI from treating
        // a repository that is still MERGING as an ordinary resolved conflict.
        let merge_in_progress = git2::Repository::open(project_root)
            .map(|repo| repo.state() != git2::RepositoryState::Clean)
            .unwrap_or(true);
        if merge_in_progress {
            let aborted = remote::merge_abort(project_root)?;
            if !aborted.success {
                anyhow::bail!(
                    "merge failed: {}; automatic merge abort also failed: {}",
                    merged.message(),
                    aborted.message()
                );
            }
        }
        return Ok(RejoinOutcome::Conflict(merged.message()));
    }

    let removed = teardown_lane(project_root, lane_path, false)?;
    if !removed.success {
        return Ok(RejoinOutcome::MergedLaneRetained(removed.message()));
    }
    Ok(RejoinOutcome::Merged)
}

/// Has the Solo already brought `target` into its branch — so a Rejoin would
/// merge cleanly? True when the lane is settled (not mid-merge, nothing
/// uncommitted) and the target branch's tip is contained in the lane's HEAD.
/// Drives the conflict card's flip to "ready" after the agent resolves.
pub fn rejoin_ready(lane_path: &Path, target: &str) -> Result<bool> {
    let repo = git2::Repository::open(lane_path)?;
    if repo.state() != git2::RepositoryState::Clean {
        return Ok(false);
    }
    if lane_is_dirty(lane_path)? {
        return Ok(false);
    }
    let head = repo.head()?.peel_to_commit()?.id();
    let Ok(reference) = repo.resolve_reference_from_short_name(target) else {
        return Ok(false);
    };
    let target_tip = reference.peel_to_commit()?.id();
    Ok(head == target_tip || repo.graph_descendant_of(head, target_tip)?)
}

/// The conflicted paths in a failed merge's output — the "CONFLICT (…):" lines
/// git prints. Content/add conflicts name the file after "Merge conflict in";
/// delete-flavored ones lead with the path. Lines this can't parse are simply
/// skipped — an empty result means the caller should show git's message as-is.
pub fn conflicted_paths(merge_output: &str) -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    for line in merge_output.lines() {
        let Some(rest) = line.trim().strip_prefix("CONFLICT (") else {
            continue;
        };
        let Some((_, detail)) = rest.split_once("): ") else {
            continue;
        };
        let path = match detail.split_once("Merge conflict in ") {
            Some((_, path)) => path.trim(),
            None => match detail.split_once(" deleted in ") {
                Some((path, _)) => path.trim(),
                None => continue,
            },
        };
        if !path.is_empty() && !paths.iter().any(|existing| existing == path) {
            paths.push(path.to_string());
        }
    }
    paths
}

/// Remove the lane's folder (worktree + bookkeeping). Without `force` git
/// refuses on uncommitted changes — the caller's confirm dialog is the front
/// door, this refusal is the backstop. A lane folder already gone just prunes.
pub fn teardown_lane(project_root: &Path, lane_path: &Path, force: bool) -> Result<RemoteOutput> {
    if !lane_path.exists() {
        let _ = remote::worktree_prune(project_root);
        return Ok(RemoteOutput {
            success: true,
            stdout: String::new(),
            stderr: String::new(),
        });
    }
    // Generated doc snapshots are protected from accidental agent edits while
    // the lane is live. Unlock only those managed directories so Git can tear
    // the disposable worktree down normally.
    let _ = unlock_choro_docs_snapshot(lane_path);
    let output = remote::worktree_remove(project_root, lane_path, force)?;
    if output.success {
        let _ = remote::worktree_prune(project_root);
        if lane_path.exists() {
            fs::remove_dir_all(lane_path).context("failed to remove leftover lane folder")?;
        }
    }
    Ok(output)
}

/// Resolve a sub-app's `rel_path` ("." for the root) under `root`.
fn sub_app_dir(root: &Path, rel_path: &str) -> PathBuf {
    if rel_path == "." {
        root.to_path_buf()
    } else {
        root.join(rel_path)
    }
}

fn copy_env_files(project_root: &Path, lane_path: &Path) -> LaneStepResult {
    let mut copied = 0usize;
    let mut failures = Vec::new();
    for sub_app in detect_project_services(project_root) {
        let src_dir = sub_app_dir(project_root, &sub_app.rel_path);
        let dst_dir = sub_app_dir(lane_path, &sub_app.rel_path);
        for name in &sub_app.env_files {
            let src = src_dir.join(name);
            let dst = dst_dir.join(name);
            // A tracked env file already exists in the worktree; keep it.
            if !src.is_file() || dst.exists() {
                continue;
            }
            if let Err(error) =
                fs::create_dir_all(&dst_dir).and_then(|_| fs::copy(&src, &dst).map(|_| ()))
            {
                failures.push(format!("{name}: {error}"));
                continue;
            }
            copied += 1;
        }
    }
    if failures.is_empty() {
        LaneStepResult::ok(LaneStep::EnvFiles, format!("copied {copied}"))
    } else {
        LaneStepResult::failed(LaneStep::EnvFiles, failures.join("; "))
    }
}

fn clone_dependencies(project_root: &Path, lane_path: &Path) -> LaneStepResult {
    let mut dirs: Vec<String> = vec![".".to_string()];
    for sub_app in detect_project_services(project_root) {
        if !dirs.contains(&sub_app.rel_path) {
            dirs.push(sub_app.rel_path);
        }
    }

    let mut cloned = 0usize;
    let mut plain_copied = false;
    let mut failures = Vec::new();
    for rel in &dirs {
        let src = sub_app_dir(project_root, rel).join("node_modules");
        let dst = sub_app_dir(lane_path, rel).join("node_modules");
        if !src.is_dir() || dst.exists() {
            continue;
        }
        match clone_dir(&src, &dst) {
            Ok(CloneMode::Clone) => cloned += 1,
            Ok(CloneMode::PlainCopy) => {
                cloned += 1;
                plain_copied = true;
            }
            Err(error) => failures.push(format!("{rel}: {error:#}")),
        }
    }

    if !failures.is_empty() {
        LaneStepResult::failed(LaneStep::Dependencies, failures.join("; "))
    } else if plain_copied {
        LaneStepResult::ok(LaneStep::Dependencies, format!("copied {cloned}"))
    } else {
        LaneStepResult::ok(LaneStep::Dependencies, format!("cloned {cloned}"))
    }
}

enum CloneMode {
    /// APFS copy-on-write — seconds, near-zero disk until files diverge.
    Clone,
    /// Plain recursive copy — the non-APFS fallback.
    PlainCopy,
}

fn clone_dir(src: &Path, dst: &Path) -> Result<CloneMode> {
    let clone = Command::new("cp")
        .arg("-c")
        .arg("-R")
        .arg(src)
        .arg(dst)
        .output()
        .context("failed to run cp")?;
    if clone.status.success() {
        return Ok(CloneMode::Clone);
    }
    // A partial clone attempt may leave a half-written target behind.
    if dst.exists() {
        let _ = fs::remove_dir_all(dst);
    }
    let copy = Command::new("cp")
        .arg("-R")
        .arg(src)
        .arg(dst)
        .output()
        .context("failed to run cp")?;
    if copy.status.success() {
        Ok(CloneMode::PlainCopy)
    } else {
        anyhow::bail!(
            "copy failed: {}",
            String::from_utf8_lossy(&copy.stderr).trim()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::read::fixtures::*;

    fn lane_dir() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let lane = dir.path().join("lane");
        (dir, lane)
    }

    #[test]
    fn rejoin_ready_tracks_whether_target_is_merged_into_the_lane() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        configure_identity(&repo);
        let base = crate::git::read_head(dir.path()).unwrap().branch.unwrap();
        let (_keep, lane) = lane_dir();
        let created = create_lane(
            dir.path(),
            &lane,
            "solo/ready",
            Some(&base),
            LaneProfile::CodeOnly,
        );
        assert!(created.iter().all(|result| result.ok), "{created:?}");

        // Forked at the target's tip: nothing to bring in, ready by definition.
        assert!(rejoin_ready(&lane, &base).unwrap());

        // The main tree moves ahead — the lane no longer contains the target.
        std::fs::write(dir.path().join("main.txt"), "mainline\n").unwrap();
        commit_all(&repo, "mainline work");
        assert!(!rejoin_ready(&lane, &base).unwrap());

        // The Solo merges the target in (what the resolve directive asks for).
        let lane_repo = git2::Repository::open(&lane).unwrap();
        configure_identity(&lane_repo);
        let merged = remote::merge(&lane, &base).unwrap();
        assert!(merged.success, "{}", merged.message());
        assert!(rejoin_ready(&lane, &base).unwrap());

        // Dirty lane work blocks readiness until committed.
        std::fs::write(lane.join("wip.txt"), "wip\n").unwrap();
        assert!(!rejoin_ready(&lane, &base).unwrap());
    }

    #[test]
    fn conflicted_paths_parses_git_merge_output() {
        let output = "Auto-merging README.md\n\
            CONFLICT (content): Merge conflict in README.md\n\
            Auto-merging public/index.html\n\
            CONFLICT (content): Merge conflict in public/index.html\n\
            CONFLICT (modify/delete): src/old.rs deleted in HEAD and modified \
            in solo/hi. Version solo/hi of src/old.rs left in tree.\n\
            Automatic merge failed; fix conflicts and then commit the result.";
        assert_eq!(
            conflicted_paths(output),
            vec!["README.md", "public/index.html", "src/old.rs"]
        );
        assert!(conflicted_paths("error: could not switch branch").is_empty());
    }

    #[test]
    fn lane_path_for_is_deterministic_per_agent() {
        let project = ProjectId(Uuid::parse_str("00000000-0000-0000-0000-0000000000aa").unwrap());
        let agent = Uuid::parse_str("00000000-0000-0000-0000-0000000000bb").unwrap();
        let other = Uuid::parse_str("00000000-0000-0000-0000-0000000000cc").unwrap();
        assert_eq!(lane_path_for(project, agent), lane_path_for(project, agent));
        assert_ne!(lane_path_for(project, agent), lane_path_for(project, other));
        assert!(lane_path_for(project, agent).starts_with(lane_root()));
    }

    #[test]
    fn code_only_lane_materializes_worktree_and_skips_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = repo_with_commit(dir.path());
        let base = crate::git::read_head(dir.path()).unwrap().branch.unwrap();
        let (_keep, lane) = lane_dir();

        let results = create_lane(
            dir.path(),
            &lane,
            "solo/quick",
            Some(&base),
            LaneProfile::CodeOnly,
        );
        assert_eq!(results.len(), 4);
        assert!(results.iter().all(|result| result.ok), "{results:?}");
        assert_eq!(results[2].detail, "skipped");
        assert_eq!(results[3].detail, "skipped");
        assert!(lane.join("README.md").exists());
        assert!(!lane_is_dirty(&lane).unwrap());
    }

    #[test]
    fn solo_docs_snapshot_is_read_only_refreshable_and_git_invisible() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = repo_with_commit(dir.path());
        let docs = dir.path().join(DOCS_DIR_NAME);
        std::fs::create_dir_all(docs.join("feature.assets")).unwrap();
        std::fs::write(docs.join("feature.choro"), "version one\n").unwrap();
        std::fs::write(docs.join("feature.assets/mock.png"), b"png").unwrap();
        std::fs::write(docs.join(".metadata.json"), "{}\n").unwrap();

        let base = crate::git::read_head(dir.path()).unwrap().branch.unwrap();
        let (_keep, lane) = lane_dir();
        let results = create_lane(
            dir.path(),
            &lane,
            "solo/docs",
            Some(&base),
            LaneProfile::CodeOnly,
        );
        assert!(results.iter().all(|result| result.ok), "{results:?}");
        assert_eq!(
            std::fs::read_to_string(lane.join("choro_docs/feature.choro")).unwrap(),
            "version one\n"
        );
        assert!(lane.join("choro_docs/feature.assets/mock.png").is_file());
        assert!(!lane.join("choro_docs/.metadata.json").exists());
        assert!(std::fs::metadata(lane.join("choro_docs/feature.choro"))
            .unwrap()
            .permissions()
            .readonly());
        assert!(!lane_is_dirty(&lane).unwrap());

        std::fs::write(docs.join("renamed.choro"), "version two\n").unwrap();
        std::fs::remove_file(docs.join("feature.choro")).unwrap();
        let refreshed = refresh_choro_docs_snapshot(dir.path(), &lane);
        assert!(refreshed.ok, "{refreshed:?}");
        assert!(!lane.join("choro_docs/feature.choro").exists());
        assert_eq!(
            std::fs::read_to_string(lane.join("choro_docs/renamed.choro")).unwrap(),
            "version two\n"
        );
        assert!(!lane_is_dirty(&lane).unwrap());
    }

    #[test]
    fn full_lane_copies_env_files_and_dependencies() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        // Real-world shape: env + node_modules are gitignored, so a raw
        // worktree would miss them.
        std::fs::write(dir.path().join(".gitignore"), ".env\nnode_modules/\n").unwrap();
        std::fs::create_dir_all(dir.path().join("web")).unwrap();
        std::fs::write(dir.path().join("web/package.json"), "{\"name\":\"web\"}\n").unwrap();
        commit_all(&repo, "add web app");
        std::fs::write(dir.path().join(".env"), "ROOT=1\n").unwrap();
        std::fs::write(dir.path().join("web/.env"), "WEB=1\n").unwrap();
        std::fs::create_dir_all(dir.path().join("web/node_modules/pkg")).unwrap();
        std::fs::write(dir.path().join("web/node_modules/pkg/index.js"), "ok\n").unwrap();

        let base = crate::git::read_head(dir.path()).unwrap().branch.unwrap();
        let (_keep, lane) = lane_dir();
        let results = create_lane(
            dir.path(),
            &lane,
            "solo/full",
            Some(&base),
            LaneProfile::Full,
        );
        assert!(results.iter().all(|result| result.ok), "{results:?}");

        assert_eq!(
            std::fs::read_to_string(lane.join(".env")).unwrap(),
            "ROOT=1\n"
        );
        assert_eq!(
            std::fs::read_to_string(lane.join("web/.env")).unwrap(),
            "WEB=1\n"
        );
        assert!(lane.join("web/node_modules/pkg/index.js").exists());
        // The copied extras are ignored, so the fresh lane reads clean.
        assert!(!lane_is_dirty(&lane).unwrap());
    }

    #[test]
    fn lane_recreates_at_the_same_path_after_teardown() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = repo_with_commit(dir.path());
        let base = crate::git::read_head(dir.path()).unwrap().branch.unwrap();
        let (_keep, lane) = lane_dir();

        let created = create_lane(
            dir.path(),
            &lane,
            "solo/revive",
            Some(&base),
            LaneProfile::CodeOnly,
        );
        assert!(created.iter().all(|result| result.ok), "{created:?}");

        // Re-running while materialized is a no-op, not an error.
        let again = create_lane(
            dir.path(),
            &lane,
            "solo/revive",
            Some(&base),
            LaneProfile::CodeOnly,
        );
        assert!(again.iter().all(|result| result.ok), "{again:?}");
        assert_eq!(again[0].detail, "already prepared");

        let removed = teardown_lane(dir.path(), &lane, false).unwrap();
        assert!(removed.success, "teardown failed: {}", removed.message());
        assert!(!lane.exists());

        // Recreate from the surviving branch at the exact same path.
        let recreated = create_lane(
            dir.path(),
            &lane,
            "solo/revive",
            None,
            LaneProfile::CodeOnly,
        );
        assert!(recreated.iter().all(|result| result.ok), "{recreated:?}");
        assert_eq!(
            crate::git::read_head(&lane).unwrap().branch.as_deref(),
            Some("solo/revive")
        );
    }

    #[test]
    fn materialize_refuses_a_mismatched_worktree_at_the_lane_path() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = repo_with_commit(dir.path());
        let base = crate::git::read_head(dir.path()).unwrap().branch.unwrap();
        let (_keep, lane) = lane_dir();
        let wrong = materialize_worktree(dir.path(), &lane, "solo/wrong", Some(&base));
        assert!(wrong.ok, "initial lane failed: {}", wrong.detail);

        let refused = materialize_worktree(dir.path(), &lane, "solo/expected", Some(&base));

        assert!(!refused.ok, "mismatched lane must not be adopted");
        assert!(refused.detail.contains("isn't empty"), "{}", refused.detail);
        assert_eq!(
            crate::git::read_head(&lane).unwrap().branch.as_deref(),
            Some("solo/wrong")
        );
    }

    #[test]
    fn teardown_refuses_a_dirty_lane_without_force() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = repo_with_commit(dir.path());
        let base = crate::git::read_head(dir.path()).unwrap().branch.unwrap();
        let (_keep, lane) = lane_dir();
        let created = create_lane(
            dir.path(),
            &lane,
            "solo/dirty",
            Some(&base),
            LaneProfile::CodeOnly,
        );
        assert!(created.iter().all(|result| result.ok), "{created:?}");

        std::fs::write(lane.join("wip.txt"), "not committed\n").unwrap();
        assert!(lane_is_dirty(&lane).unwrap());

        let refused = teardown_lane(dir.path(), &lane, false).unwrap();
        assert!(!refused.success, "dirty teardown must refuse");
        assert!(lane.exists());

        let forced = teardown_lane(dir.path(), &lane, true).unwrap();
        assert!(
            forced.success,
            "forced teardown failed: {}",
            forced.message()
        );
        assert!(!lane.exists());
    }

    #[test]
    fn refuses_to_clobber_a_foreign_non_empty_lane_folder() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = repo_with_commit(dir.path());
        let base = crate::git::read_head(dir.path()).unwrap().branch.unwrap();
        let (_keep, lane) = lane_dir();
        std::fs::create_dir_all(&lane).unwrap();
        std::fs::write(lane.join("mystery.txt"), "someone else's files\n").unwrap();

        let results = create_lane(
            dir.path(),
            &lane,
            "solo/blocked",
            Some(&base),
            LaneProfile::CodeOnly,
        );
        assert_eq!(results.len(), 1);
        assert!(!results[0].ok);
        assert!(lane.join("mystery.txt").exists());
    }

    fn configure_identity(repo: &git2::Repository) {
        let mut config = repo.config().unwrap();
        config.set_str("user.name", "Test").unwrap();
        config.set_str("user.email", "test@example.com").unwrap();
    }

    #[test]
    fn rejoin_commits_lane_work_merges_home_and_tears_down() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        configure_identity(&repo);
        let base = crate::git::read_head(dir.path()).unwrap().branch.unwrap();
        let (_keep, lane) = lane_dir();
        let created = create_lane(
            dir.path(),
            &lane,
            "solo/home",
            Some(&base),
            LaneProfile::CodeOnly,
        );
        assert!(created.iter().all(|result| result.ok), "{created:?}");

        // Uncommitted lane work — rejoin must sweep it into a commit itself.
        std::fs::write(lane.join("feature.txt"), "solo work\n").unwrap();
        let lane_repo = git2::Repository::open(&lane).unwrap();
        configure_identity(&lane_repo);

        let outcome = rejoin(dir.path(), &lane, "solo/home", "Fix the thing", None).unwrap();
        assert_eq!(outcome, RejoinOutcome::Merged);
        assert!(dir.path().join("feature.txt").exists());
        assert!(!lane.exists(), "lane should be torn down after rejoin");
        // The branch survives the teardown.
        assert!(branch_exists(dir.path(), "solo/home"));
    }

    #[test]
    fn rejoin_reports_a_merged_lane_that_git_refuses_to_remove() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        configure_identity(&repo);
        let base = crate::git::read_head(dir.path()).unwrap().branch.unwrap();
        let (_keep, lane) = lane_dir();
        let created = create_lane(
            dir.path(),
            &lane,
            "solo/locked",
            Some(&base),
            LaneProfile::CodeOnly,
        );
        assert!(created.iter().all(|result| result.ok), "{created:?}");
        std::fs::write(lane.join("feature.txt"), "solo work\n").unwrap();
        let lane_repo = git2::Repository::open(&lane).unwrap();
        configure_identity(&lane_repo);
        let locked = Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(["worktree", "lock"])
            .arg(&lane)
            .output()
            .unwrap();
        assert!(
            locked.status.success(),
            "{}",
            String::from_utf8_lossy(&locked.stderr)
        );

        let outcome = rejoin(dir.path(), &lane, "solo/locked", "Locked", None).unwrap();

        let RejoinOutcome::MergedLaneRetained(message) = outcome else {
            panic!("expected retained lane, got {outcome:?}");
        };
        assert!(!message.is_empty());
        assert!(dir.path().join("feature.txt").exists());
        assert!(
            lane.exists(),
            "failed cleanup must remain tracked by the caller"
        );
    }

    #[test]
    fn rejoin_can_target_a_branch_that_is_not_checked_out() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        configure_identity(&repo);
        let base = crate::git::read_head(dir.path()).unwrap().branch.unwrap();
        // A second branch to rejoin into; create_branch switches to it, so
        // hop back to the base before forking the Solo.
        crate::git::write::create_branch(dir.path(), "develop").unwrap();
        crate::git::write::checkout_branch(dir.path(), &base).unwrap();

        let (_keep, lane) = lane_dir();
        let created = create_lane(
            dir.path(),
            &lane,
            "solo/retarget",
            Some(&base),
            LaneProfile::CodeOnly,
        );
        assert!(created.iter().all(|result| result.ok), "{created:?}");
        std::fs::write(lane.join("feature.txt"), "solo work\n").unwrap();
        let lane_repo = git2::Repository::open(&lane).unwrap();
        configure_identity(&lane_repo);

        let outcome = rejoin(
            dir.path(),
            &lane,
            "solo/retarget",
            "Retarget",
            Some("develop"),
        )
        .unwrap();
        assert_eq!(outcome, RejoinOutcome::Merged);
        // The main tree switched to the target and carries the work.
        assert_eq!(
            crate::git::read_head(dir.path()).unwrap().branch.as_deref(),
            Some("develop")
        );
        assert!(dir.path().join("feature.txt").exists());
        assert!(!lane.exists());
    }

    #[test]
    fn conflicted_rejoin_aborts_and_leaves_both_sides_intact() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        configure_identity(&repo);
        let base = crate::git::read_head(dir.path()).unwrap().branch.unwrap();
        let (_keep, lane) = lane_dir();
        let created = create_lane(
            dir.path(),
            &lane,
            "solo/clash",
            Some(&base),
            LaneProfile::CodeOnly,
        );
        assert!(created.iter().all(|result| result.ok), "{created:?}");

        std::fs::write(lane.join("README.md"), "solo version\n").unwrap();
        let lane_repo = git2::Repository::open(&lane).unwrap();
        configure_identity(&lane_repo);
        std::fs::write(dir.path().join("README.md"), "main version\n").unwrap();
        commit_all(&repo, "main: rewrite readme");

        let outcome = rejoin(dir.path(), &lane, "solo/clash", "Clash", None).unwrap();
        let RejoinOutcome::Conflict(message) = outcome else {
            panic!("expected conflict, got {outcome:?}");
        };
        assert!(!message.is_empty());
        // Main tree restored to clean; lane untouched and still materialized.
        let snapshot = crate::git::read_snapshot(dir.path()).unwrap();
        assert!(snapshot.entries.is_empty());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("README.md")).unwrap(),
            "main version\n"
        );
        assert!(lane.exists());
        assert_eq!(
            std::fs::read_to_string(lane.join("README.md")).unwrap(),
            "solo version\n"
        );
    }

    #[test]
    fn solo_branch_names_are_slugged_and_race_free() {
        let first_id = Uuid::parse_str("00000000-0000-0000-0000-0000000000aa").unwrap();
        let second_id = Uuid::parse_str("11111111-0000-0000-0000-0000000000bb").unwrap();

        let first = solo_branch_name("Fix OAuth token refresh!", first_id);
        let second = solo_branch_name("Fix OAuth token refresh!", second_id);
        assert_eq!(
            first,
            "solo/fix-oauth-token-refresh-000000000000000000000000000000aa"
        );
        assert_eq!(
            second,
            "solo/fix-oauth-token-refresh-111111110000000000000000000000bb"
        );
        assert_ne!(first, second);
        assert_eq!(
            solo_branch_name("  !!  ", first_id),
            "solo/work-000000000000000000000000000000aa"
        );
    }

    #[test]
    fn ios_projects_default_to_code_only() {
        let ios = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(ios.path().join("ios")).unwrap();
        assert_eq!(default_profile(ios.path()), LaneProfile::CodeOnly);

        let pods = tempfile::tempdir().unwrap();
        std::fs::write(pods.path().join("Podfile"), "platform :ios\n").unwrap();
        assert_eq!(default_profile(pods.path()), LaneProfile::CodeOnly);

        let web = tempfile::tempdir().unwrap();
        std::fs::write(web.path().join("package.json"), "{}\n").unwrap();
        assert_eq!(default_profile(web.path()), LaneProfile::Full);
    }
}
