use super::*;
use crate::{
    agent_changes::MutationEvidence,
    delegation::workspace::safe_path,
    git::{diff::diff_from_contents, LineOrigin},
};
use anyhow::{ensure, Context, Result};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Component, Path},
    sync::atomic::{AtomicBool, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

pub fn review_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub fn content_hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn check_running(cancel: &AtomicBool, deadline: u64) -> Result<()> {
    ensure!(
        !cancel.load(Ordering::Acquire),
        "Review preparation cancelled"
    );
    ensure!(
        review_now() < deadline,
        "Review deadline reached during preparation"
    );
    Ok(())
}

pub fn excluded_review_path(path: &Path) -> bool {
    path.components().any(|c| match c {
        Component::Normal(part) => {
            let p = part.to_string_lossy().to_ascii_lowercase();
            matches!(
                p.as_str(),
                ".git"
                    | "node_modules"
                    | "target"
                    | "dist"
                    | "build"
                    | ".next"
                    | ".choro"
                    | ".codex"
                    | ".claude"
                    | "__pycache__"
            ) || (p.starts_with(".env") && !p.ends_with(".example") && !p.ends_with(".sample"))
                || matches!(
                    p.as_str(),
                    "credentials"
                        | "credentials.json"
                        | "secrets.json"
                        | "id_rsa"
                        | "id_ed25519"
                        | ".npmrc"
                        | ".netrc"
                        | ".git-credentials"
                        | ".pypirc"
                        | ".aws"
                        | ".ssh"
                        | ".gnupg"
                        | ".kube"
                )
                || p.ends_with(".pem")
                || p.ends_with(".p12")
                || p.ends_with(".key")
        }
        _ => true,
    })
}
pub fn repository_identity(root: &Path) -> Result<String> {
    let repo = git2::Repository::open(root)
        .context("Review requires the conversation's Git repository")?;
    let head = repo.head().ok();
    let value = format!(
        "{}\n{}\n{}\n{}",
        root.canonicalize()?.display(),
        repo.path().canonicalize()?.display(),
        head.as_ref()
            .and_then(|h| h.name().ok())
            .unwrap_or_default(),
        head.as_ref()
            .and_then(|h| h.target())
            .map(|id| id.to_string())
            .unwrap_or_default()
    );
    Ok(content_hash(value.as_bytes()))
}
pub fn read_review_blob(storage: &Path, hash: &str) -> Result<String> {
    ensure!(
        hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid content identity"
    );
    let path = storage.join("blobs").join(hash);
    let meta = fs::symlink_metadata(&path)?;
    ensure!(
        meta.is_file() && !meta.file_type().is_symlink() && meta.len() <= MAX_SOURCE_BYTES as u64,
        "Unsafe review blob"
    );
    let bytes = fs::read(path)?;
    ensure!(
        content_hash(&bytes) == hash,
        "Snapshot content identity changed"
    );
    Ok(String::from_utf8(bytes)?)
}
fn put_blob(storage: &Path, text: &str) -> Result<String> {
    ensure!(
        text.len() <= MAX_SOURCE_BYTES,
        "Source exceeds per-file limit"
    );
    let hash = content_hash(text.as_bytes());
    fs::create_dir_all(storage.join("blobs"))?;
    let path = storage.join("blobs").join(&hash);
    match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(mut f) => {
            f.write_all(text.as_bytes())?;
            f.sync_all()?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            read_review_blob(storage, &hash)?;
        }
        Err(e) => return Err(e.into()),
    }
    Ok(hash)
}

/// Preserve a patch's actual numbered lines without filling unrecorded gaps or
/// borrowing another writer's current contents as the historical file version.
fn patch_sources(diff: &crate::git::FileDiff, path: &Path) -> Result<(String, String)> {
    ensure!(diff.path == path && !diff.is_binary && !diff.hunks.is_empty(),
        "Recorded patch path or contents are unavailable");
    let mut before = BTreeMap::<u32, String>::new();
    let mut after = BTreeMap::<u32, String>::new();
    let mut changed = false;
    for hunk in &diff.hunks {
        for line in &hunk.lines {
            // Provider diffs may already have stripped LF but retained CR.
            // Match the normalized excerpts returned for full source lines.
            let text = line.text.strip_suffix('\n').unwrap_or(&line.text);
            let text = text.strip_suffix('\r').unwrap_or(text);
            ensure!(!text.contains(['\n', '\0']), "Invalid recorded patch line");
            ensure!(match line.origin {
                LineOrigin::Add => line.old_no.is_none() && line.new_no.is_some(),
                LineOrigin::Remove => line.old_no.is_some() && line.new_no.is_none(),
                LineOrigin::Context => line.old_no.is_some() && line.new_no.is_some(),
            }, "Recorded patch has invalid line positions");
            changed |= line.origin != LineOrigin::Context;
            for (map, number) in [(&mut before, line.old_no), (&mut after, line.new_no)] {
                if let Some(number) = number {
                    ensure!(number > 0, "Recorded patch line numbers must be positive");
                    if let Some(old) = map.insert(number, text.to_string()) {
                        ensure!(old == text, "Recorded patch has conflicting line contents");
                    }
                }
            }
        }
    }
    ensure!(changed, "Recorded patch has no attributed changes");
    let before = serde_json::to_string(&before)?;
    let after = serde_json::to_string(&after)?;
    ensure!(before.len() <= MAX_SOURCE_BYTES && after.len() <= MAX_SOURCE_BYTES,
        "Recorded patch source exceeds 2 MiB");
    Ok((before, after))
}

#[derive(PartialEq, Eq)]
struct Capture {
    identity: String,
    source: BTreeMap<PathBuf, String>,
    omitted: BTreeMap<PathBuf, String>,
    bytes: usize,
}
fn record_walk_error(root: &Path, error: &ignore::Error, omitted: &mut BTreeMap<PathBuf, String>) {
    match error {
        ignore::Error::Partial(errors) => {
            for error in errors { record_walk_error(root, error, omitted); }
        }
        ignore::Error::WithPath { path, .. } => {
            let relative = path.strip_prefix(root).ok().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
            omitted.insert(relative.into(), "Source paths could not be enumerated".into());
        }
        ignore::Error::WithDepth { err, .. } | ignore::Error::WithLineNumber { err, .. } => record_walk_error(root, err, omitted),
        _ => { omitted.insert(".".into(), "Source paths could not be enumerated".into()); }
    }
}
fn collect(
    root: &Path,
    storage: &Path,
    required: &BTreeSet<PathBuf>,
    source_budget: usize,
    cancel: &AtomicBool,
    deadline: u64,
) -> Result<Capture> {
    collect_with_read(root, storage, required, source_budget, cancel, deadline, |root, path| {
        let mut bytes = vec![];
        open_contained_file(root, path)?.take((MAX_SOURCE_BYTES + 1) as u64).read_to_end(&mut bytes)?;
        Ok(bytes)
    })
}
fn collect_with_read(
    root: &Path,
    storage: &Path,
    required: &BTreeSet<PathBuf>,
    source_budget: usize,
    cancel: &AtomicBool,
    deadline: u64,
    read: impl Fn(&Path, &Path) -> Result<Vec<u8>>,
) -> Result<Capture> {
    let identity = repository_identity(root)?;
    let mut paths = BTreeSet::new();
    let mut omitted = BTreeMap::new();
    let walk_root = root.to_path_buf();
    let walk = ignore::WalkBuilder::new(root)
        .hidden(false)
        .follow_links(false)
        .filter_entry(move |entry| {
            entry.depth() == 0
                || !excluded_review_path(
                    entry
                        .path()
                        .strip_prefix(&walk_root)
                        .unwrap_or(entry.path()),
                )
        })
        .build();
    for entry in walk {
        check_running(cancel, deadline)?;
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => { record_walk_error(root, &error, &mut omitted); continue; }
        };
        if entry.file_type().is_some_and(|ft| !ft.is_dir()) {
            paths.insert(entry.path().strip_prefix(root)?.to_path_buf());
        }
    }
    // A tracked/owned file must be accounted even if an ignore rule hides it.
    paths.extend(required.iter().cloned());
    // Tracked source remains surrounding evidence even if a newer ignore rule
    // hides it from the walk. Metadata/artifact/credential exclusions still win.
    let repo = git2::Repository::open(root)?;
    for entry in repo.index()?.iter() {
        if let Ok(path) = std::str::from_utf8(&entry.path) {
            paths.insert(PathBuf::from(path));
        }
    }
    let mut source = BTreeMap::new();
    let mut used = 0;
    let mut paths: Vec<_> = paths.into_iter().collect();
    paths.sort_by(|a, b| (!required.contains(a), a).cmp(&(!required.contains(b), b)));
    for path in paths {
        check_running(cancel, deadline)?;
        if excluded_review_path(&path) {
            omitted.insert(
                path,
                "Excluded credential, metadata or generated artifact".into(),
            );
            continue;
        }
        let absolute = match safe_path(root, &path) {
            Ok(p) => p,
            Err(_) => {
                omitted.insert(path, "Unsafe path or symlink".into());
                continue;
            }
        };
        let meta = match fs::symlink_metadata(&absolute) {
            Ok(m) => m,
            Err(error) => {
                let reason = if error.kind() == std::io::ErrorKind::NotFound { "Source missing" } else { "Source unavailable" };
                omitted.insert(path, reason.into());
                continue;
            }
        };
        let reason = if meta.file_type().is_symlink() || !meta.is_file() {
            Some("Symlink or non-regular source")
        } else if meta.len() > MAX_SOURCE_BYTES as u64 {
            Some("Source exceeds 2 MiB")
        } else if used + meta.len() as usize > source_budget {
            Some("Source exceeds 100 MiB review budget")
        } else {
            None
        };
        if let Some(reason) = reason {
            omitted.insert(path, reason.into());
            continue;
        }
        let bytes = match read(root, &path) {
            Ok(bytes) => bytes,
            Err(_) => { omitted.insert(path, "Source could not be read".into()); continue; }
        };
        if bytes.len() > MAX_SOURCE_BYTES {
            omitted.insert(path, "Source exceeds 2 MiB".into());
            continue;
        }
        let text = if bytes.contains(&0) {
            None
        } else {
            String::from_utf8(bytes).ok()
        };
        match text {
            Some(text) => {
                if used + text.len() > source_budget {
                    omitted.insert(path, "Source exceeds 100 MiB review budget".into());
                    continue;
                }
                used += text.len();
                source.insert(path, put_blob(storage, &text)?);
            }
            None => {
                omitted.insert(path, "Binary or non-UTF-8 source".into());
            }
        }
    }
    ensure!(
        identity == repository_identity(root)?,
        "Repository identity changed during capture"
    );
    Ok(Capture {
        identity,
        source,
        omitted,
        bytes: used,
    })
}

/// Call on a worker thread. Immutable blob storage must be in Choro app data,
/// outside the workspace. No files in the workspace are changed or removed.
pub fn prepare_review(
    run: &mut ReviewRun,
    root: &Path,
    storage: &Path,
    requirements: ReviewRequirements,
    evidence: &[MutationEvidence],
    ownership_limitations: &[String],
    cancel: &AtomicBool,
) -> Result<ReviewInput> {
    let root = root.canonicalize()?;
    fs::create_dir_all(storage)?;
    ensure!(
        !storage.canonicalize()?.starts_with(&root),
        "Review snapshots must be outside the repository"
    );
    check_running(cancel, run.deadline_at)?;
    ensure!(
        run.state == ReviewRunState::Preparing && run.files.is_empty(),
        "Review input is prepared once"
    );
    let mut owned: Vec<MutationEvidence> = vec![];
    let mut last_by_path = BTreeMap::<PathBuf, usize>::new();
    for e in evidence.iter().filter(|e| {
        e.confirmed
            && e.key.agent_id == run.parent_id
            && e.key.project_id == run.project_id
            && crate::agent_changes::working_directory(&e.key.root) == root
    }) {
        // Collapse only a proven contiguous content chain. Shared-file gaps
        // remain separate attributed receipts, never a whole-workspace diff.
        if let Some(index) = last_by_path.get(&e.key.path).copied() {
            let previous = &mut owned[index];
            if previous.before.is_some() && previous.after.is_some()
                && e.after.is_some() && previous.after == e.before {
                previous.after = e.after.clone();
                previous.after_hash = e.after_hash.clone();
                previous.captured_at = e.captured_at;
                continue;
            }
        }
        last_by_path.insert(e.key.path.clone(), owned.len());
        owned.push(e.clone());
    }
    let evidence: Vec<_> = owned.iter().collect();
    let required = evidence.iter().map(|e| e.key.path.clone()).collect();
    // Reserve inspectable mutation contents first. A large repository must
    // not consume the entire budget before its actual changes can be reviewed.
    let reserved = evidence
        .iter()
        .filter(|e| !excluded_review_path(&e.key.path))
        .map(|e| match e.before.as_ref().zip(e.after.as_ref()) {
            Some((before, after)) if before.len() <= MAX_SOURCE_BYTES
                && after.len() <= MAX_SOURCE_BYTES && !before.contains('\0')
                && !after.contains('\0') => before.len().saturating_add(after.len()),
            None => e.patch.as_ref().and_then(|p| serde_json::to_vec(p).ok())
                .map_or(0, |bytes| bytes.len().saturating_mul(2)),
            _ => 0,
        })
        .fold(0usize, usize::saturating_add);
    ensure!(serde_json::to_vec(&requirements)?.len() <= MAX_REVIEW_CONTEXT_BYTES,
        "Frozen conversation context exceeds 4 MiB; review a focused coding conversation. No requirements were silently omitted");
    let captured = capture_stable(|| {
        check_running(cancel, run.deadline_at)?;
        collect(
            &root,
            storage,
            &required,
            MAX_REVIEW_BYTES.saturating_sub(reserved),
            cancel,
            run.deadline_at,
        )
    })?;
    let source_bytes = captured.bytes;
    let owned = super::replay::collapse_current_edits(owned, &captured.source, storage);
    let evidence: Vec<_> = owned.iter().collect();
    let mut input = ReviewInput {
        version: REVIEW_VERSION,
        snapshot_id: run.snapshot_id,
        root,
        repository_identity: captured.identity,
        requirements,
        source: captured.source,
        omitted_source: captured.omitted,
        diffs: BTreeMap::new(),
        batches: vec![],
        patch_only_files: BTreeSet::new(),
    };
    run.limitations.extend_from_slice(ownership_limitations);
    for (path, reason) in &input.omitted_source {
        if reason.contains("exceeds") || matches!(reason.as_str(), "Source could not be read" | "Source paths could not be enumerated" | "Source unavailable") {
            run.limitations
                .push(format!("Surrounding source {}: {reason}", path.display()));
        }
    }
    let mut seen = BTreeSet::new();
    let mut budget = source_bytes;
    for e in evidence {
        check_running(cancel, run.deadline_at)?;
        let id = content_hash(serde_json::to_string(&e.key)?.as_bytes());
        if !seen.insert(id.clone()) {
            continue;
        }
        let mut f = ReviewFile {
            id: id.clone(),
            path: e.key.path.clone(),
            change_kind: "Modified".into(),
            attributed_ranges: vec![],
            before_hash: None,
            after_hash: None,
            diff_pages: 0,
            consumed_pages: BTreeSet::new(),
            status: ReviewFileStatus::Pending,
            skip_reason: None,
        };
        let before = e.before.as_deref();
        let after = e.after.as_deref();
        let patch = if before.is_none() || after.is_none() {
            e.patch.as_ref().map(|p| patch_sources(p, &f.path))
        } else { None };
        let patch_error = patch.as_ref().and_then(|result| result.as_ref().err())
            .map(|error| format!("Invalid recorded patch: {error:#}"));
        let reason =
            if excluded_review_path(&f.path) {
                Some("Excluded credential, metadata or generated artifact")
            } else if safe_path(&input.root, &f.path).is_err() {
                Some("Unsafe path or symlink")
            } else if input.omitted_source.get(&f.path).is_some_and(|r| {
                r == "Symlink or non-regular source" || r == "Unsafe path or symlink"
            }) {
                Some("Unsafe path or symlink")
            } else if let Some(error) = patch_error.as_deref() {
                Some(error)
            } else if (before.is_none() || after.is_none()) && patch.is_none() {
                Some("Confirmed mutation lacks inspectable before/after contents")
            } else if patch.is_some() {
                None // The recorded hunk views were validated and bounded above.
            } else if before.unwrap().len() > MAX_SOURCE_BYTES
                || after.unwrap().len() > MAX_SOURCE_BYTES
            {
                Some("Mutation source exceeds 2 MiB")
            } else if budget + before.unwrap().len() + after.unwrap().len() > MAX_REVIEW_BYTES {
                Some("Mutation evidence exceeds 100 MiB")
            } else if before.unwrap().contains('\0') || after.unwrap().contains('\0') {
                Some("Binary mutation")
            } else if e
                .before_hash
                .as_ref()
                .is_some_and(|h| h != &content_hash(before.unwrap().as_bytes()))
                || e.after_hash
                    .as_ref()
                    .is_some_and(|h| h != &content_hash(after.unwrap().as_bytes()))
            {
                Some("Mutation contents do not match recorded identity")
            } else {
                None
            };
        if let Some(reason) = reason {
            f.status = ReviewFileStatus::Skipped;
            f.skip_reason = Some(reason.into());
            run.limitations
                .push(format!("{}: {reason}", f.path.display()));
            run.files.push(f);
            continue;
        }
        let patch_only = patch.is_some();
        let patch_contents = patch.transpose()?;
        let (before, after) = match patch_contents.as_ref() {
            Some((before, after)) => (before.as_str(), after.as_str()),
            None => (before.unwrap(), after.unwrap()),
        };
        if budget.saturating_add(before.len()).saturating_add(after.len()) > MAX_REVIEW_BYTES {
            f.status = ReviewFileStatus::Skipped;
            f.skip_reason = Some("Mutation evidence exceeds 100 MiB".into());
            run.limitations.push(format!("{}: Mutation evidence exceeds 100 MiB", f.path.display()));
            run.files.push(f);
            continue;
        }
        budget += before.len() + after.len();
        f.before_hash = Some(put_blob(storage, before)?);
        f.after_hash = Some(put_blob(storage, after)?);
        if patch_only { input.patch_only_files.insert(id.clone()); }
        // No recorded lines on one side means unknown historical contents,
        // not proof that the entire file was added or deleted.
        f.change_kind = if patch_only {
            "Recorded patch"
        } else if before == after {
            "Unchanged"
        } else if before.is_empty() {
            "Added"
        } else if after.is_empty() {
            "Deleted"
        } else {
            "Modified"
        }
        .into();
        // Each receipt's own patch is retained. Never take a diff of an entire
        // shared dirty file or collapse a gap in the mutation evidence chain.
        let diff = if patch_only { e.patch.as_ref().unwrap().clone() }
            else { diff_from_contents(&f.path, before, after)? };
        let mut text = format!(
            "File {} · {}\nBefore {}\nAfter {}\nLines: +after: added, -before: removed, old/new: context.\n",
            f.id,
            f.path.display(),
            f.before_hash.as_ref().unwrap(),
            f.after_hash.as_ref().unwrap()
        );
        if patch_only {
            text.push_str("Before/After identities contain only recorded patch lines at their original numbers. Unknown ranges were not captured; Source reads contain current frozen surrounding code.\n");
        }
        for h in diff.hunks {
            text.push_str(&h.header);
            text.push('\n');
            for line in h.lines {
                let (sign, side, number) = match line.origin {
                    LineOrigin::Add => ('+', ReviewSide::After, line.new_no),
                    LineOrigin::Remove => ('-', ReviewSide::Before, line.old_no),
                    LineOrigin::Context => (' ', ReviewSide::Source, None),
                };
                if let Some(n) = number {
                    f.attributed_ranges.push(ReviewRange {
                        side,
                        start: n,
                        end: n,
                    });
                }
                let number = match line.origin {
                    LineOrigin::Add => line.new_no.map(|n|n.to_string()).unwrap_or_default(),
                    LineOrigin::Remove => line.old_no.map(|n|n.to_string()).unwrap_or_default(),
                    LineOrigin::Context => format!("{}/{}",line.old_no.map(|n|n.to_string()).unwrap_or_default(),line.new_no.map(|n|n.to_string()).unwrap_or_default()),
                };
                text.push_str(&format!("{sign}{number}: {}\n",line.text.trim_end_matches('\n')));
            }
        }
        let pages = paginate_diff(&text)
            .into_iter()
            .map(|page| {
                Ok(ReviewDiffPage {
                    content_hash: put_blob(storage, &page)?,
                    characters: page.chars().count(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        f.diff_pages = pages.len();
        input.diffs.insert(id, pages);
        let current_matches = if patch_only {
            let lines: BTreeMap<u32, String> = serde_json::from_str(after)?;
            if lines.is_empty() { !input.source.contains_key(&f.path) }
            else if let Some(hash) = input.source.get(&f.path) {
                let source = read_review_blob(storage, hash)?;
                let source_lines: Vec<_> = source.lines().collect();
                lines.iter().all(|(number, text)| source_lines.get(*number as usize - 1)
                    .is_some_and(|line| *line == text.as_str()))
            } else { false }
        } else {
            input.source.get(&f.path) == f.after_hash.as_ref()
                || after.is_empty() && !input.source.contains_key(&f.path)
                    && input.omitted_source.get(&f.path).is_none_or(|reason| reason == "Source missing")
        };
        if !current_matches {
            let reason = format!("{} has subsequent or unavailable source beyond this mutation; receipt and current source are shown separately", f.path.display());
            if !run.limitations.contains(&reason) { run.limitations.push(reason); }
        }
        run.files.push(f);
    }
    if run.files.is_empty() {
        run.limitations
            .push("No inspectable confirmed conversation mutations were available".into());
    }
    run.files
        .sort_by(|a, b| a.path.cmp(&b.path).then(a.id.cmp(&b.id)));
    let mut current = ReviewBatch {
        id: 0,
        group: String::new(),
        file_ids: vec![],
    };
    let mut chars = 0;
    for file in run
        .files
        .iter()
        .filter(|f| f.status != ReviewFileStatus::Skipped)
    {
        let group = file
            .path
            .parent()
            .unwrap_or(Path::new(""))
            .display()
            .to_string();
        let length: usize = input.diffs[&file.id].iter().map(|p| p.characters).sum();
        if !current.file_ids.is_empty()
            && (current.group != group
                || current.file_ids.len() >= 5
                || chars + length > DIFF_PAGE_CHARS)
        {
            input.batches.push(current);
            current = ReviewBatch {
                id: input.batches.len(),
                group: String::new(),
                file_ids: vec![],
            };
            chars = 0;
        }
        current.group = group;
        current.file_ids.push(file.id.clone());
        chars += length;
    }
    if !current.file_ids.is_empty() {
        input.batches.push(current);
    }
    run.set_scope_deadline(&input);
    run.freshness = ReviewFreshness::Current;
    run.state = ReviewRunState::Running;
    run.revision += 1;
    Ok(input)
}
fn capture_stable(mut capture: impl FnMut() -> Result<Capture>) -> Result<Capture> {
    let mut last_error = None;
    for _ in 0..3 {
        match (capture(), capture()) {
            (Ok(a), Ok(b)) if a == b => return Ok(a),
            (Err(error), _) | (_, Err(error)) => last_error = Some(error),
            _ => {}
        }
    }
    anyhow::bail!("Snapshot preparation could not stabilize after three attempts. Stop the active writer, check source access and Review again. {}",last_error.map(|e|e.to_string()).unwrap_or_default());
}

#[cfg(test)]
mod capture_tests {
    use super::*;
    #[test]
    fn an_individual_read_error_preserves_other_frozen_source() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("repo"); fs::create_dir_all(&root).unwrap();
        git2::Repository::init(&root).unwrap();
        fs::write(root.join("available.rs"), "known source\n").unwrap();
        fs::write(root.join("unreadable.rs"), "private source\n").unwrap();
        let storage = dir.path().join("snapshot");
        let capture = capture_stable(|| collect_with_read(&root,&storage,&BTreeSet::new(),MAX_REVIEW_BYTES,
            &AtomicBool::new(false),review_now()+60,|root,path| {
                if path == Path::new("unreadable.rs") { return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied).into()); }
                Ok(fs::read(root.join(path))?)
            })).unwrap();
        assert_eq!(read_review_blob(&storage,&capture.source[Path::new("available.rs")]).unwrap(), "known source\n");
        assert!(!capture.source.contains_key(Path::new("unreadable.rs")));
        assert_eq!(capture.omitted[Path::new("unreadable.rs")], "Source could not be read");
        let mut omitted = BTreeMap::new();
        record_walk_error(&root,&ignore::Error::WithPath { path:root.join("unreadable-directory"),
            err:Box::new(ignore::Error::Io(std::io::Error::from(std::io::ErrorKind::PermissionDenied))) },&mut omitted);
        assert_eq!(omitted[Path::new("unreadable-directory")], "Source paths could not be enumerated");
    }
    #[test]
    fn unstable_captures_retry_three_times_and_never_publish_mixed_source() {
        let mut calls = 0;
        let result = capture_stable(|| {
            calls += 1;
            Ok(Capture {
                identity: calls.to_string(),
                source: BTreeMap::new(),
                omitted: BTreeMap::new(),
                bytes: 0,
            })
        });
        assert!(result.is_err());
        assert_eq!(calls, 6);
        let mut calls = 0;
        let result = capture_stable(|| {
            calls += 1;
            Ok(Capture {
                identity: if calls < 3 {
                    calls.to_string()
                } else {
                    "stable".into()
                },
                source: BTreeMap::new(),
                omitted: BTreeMap::new(),
                bytes: 0,
            })
        })
        .unwrap();
        assert_eq!(calls, 4);
        assert_eq!(result.identity, "stable");
    }
}
fn paginate_diff(text: &str) -> Vec<String> {
    let mut pages = vec![];
    let mut page = String::new();
    let mut chars = 0;
    // Split by characters, including very long single lines; no invalid UTF-8
    // boundaries and no oversized indivisible patch can evade accounting.
    for ch in text.chars() {
        if chars == DIFF_PAGE_CHARS {
            pages.push(std::mem::take(&mut page));
            chars = 0;
        }
        page.push(ch);
        chars += 1;
    }
    if !page.is_empty() {
        pages.push(page);
    }
    pages
}
pub fn revalidate_review(run: &mut ReviewRun, input: &ReviewInput, mutation_revision: u64) {
    if !matches!(run.freshness, ReviewFreshness::Current) {
        return;
    }
    if mutation_revision != run.mutation_revision {
        run.freshness = ReviewFreshness::Outdated(
            "Conversation changes were recorded after review started".into(),
        );
        return;
    }
    match repository_identity(&input.root) {
        Ok(id) if id == input.repository_identity => {}
        Ok(_) => {
            run.freshness = ReviewFreshness::Outdated("Repository identity changed".into());
            return;
        }
        Err(_) => {
            run.freshness = ReviewFreshness::Uncertain("Repository identity is unavailable".into());
            return;
        }
    }
    let paths: BTreeSet<_> = run
        .files
        .iter()
        .filter(|f| f.status != ReviewFileStatus::Skipped)
        .map(|f| f.path.clone())
        .chain(run.consulted_paths.iter().cloned())
        .collect();
    for path in paths {
        let result = safe_path(&input.root, &path).and_then(|p| {
            let meta = fs::symlink_metadata(&p)?;
            ensure!(
                meta.is_file()
                    && !meta.file_type().is_symlink()
                    && meta.len() <= MAX_SOURCE_BYTES as u64,
                "Unavailable source"
            );
            let mut bytes = vec![];
            open_contained_file(&input.root, &path)?
                .take((MAX_SOURCE_BYTES + 1) as u64)
                .read_to_end(&mut bytes)?;
            ensure!(bytes.len() <= MAX_SOURCE_BYTES, "Source exceeds limit");
            Ok(content_hash(&bytes))
        });
        match (input.source.get(&path), result) {
            (Some(expected), Ok(actual)) if expected == &actual => {}
            (None, Err(_)) if !input.root.join(&path).exists() => {}
            (_, Ok(_)) => {
                run.freshness = ReviewFreshness::Outdated(format!("{} changed", path.display()));
                return;
            }
            _ => {
                run.freshness =
                    ReviewFreshness::Uncertain(format!("Cannot verify {}", path.display()));
                return;
            }
        }
    }
}

/// Resolve every component relative to a directory descriptor. O_NOFOLLOW on
/// every hop prevents a racing parent symlink from escaping the workspace.
#[cfg(unix)]
fn open_contained_file(root: &Path, relative: &Path) -> Result<fs::File> {
    use std::ffi::CString;
    use std::os::{
        fd::{AsRawFd, FromRawFd},
        unix::ffi::OsStrExt,
    };
    let mut directory = fs::File::open(root)?;
    let components: Vec<_> = relative.components().collect();
    ensure!(!components.is_empty(), "Empty source path");
    for (i, part) in components.iter().enumerate() {
        let Component::Normal(part) = part else {
            anyhow::bail!("Unsafe source path");
        };
        let name = CString::new(part.as_bytes())?;
        let flags = libc::O_RDONLY
            | libc::O_CLOEXEC
            | libc::O_NOFOLLOW
            | if i + 1 == components.len() {
                0
            } else {
                libc::O_DIRECTORY
            };
        let fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        directory = unsafe { fs::File::from_raw_fd(fd) };
    }
    ensure!(directory.metadata()?.is_file(), "Non-regular source");
    Ok(directory)
}
#[cfg(not(unix))]
fn open_contained_file(_root: &Path, _relative: &Path) -> Result<fs::File> {
    anyhow::bail!("This platform cannot enforce contained snapshot reads")
}
