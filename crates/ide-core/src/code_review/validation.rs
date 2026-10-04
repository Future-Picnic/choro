use super::*;
use anyhow::{ensure, Context, Result};
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReviewReport {
    Stage {
        stage: ReviewStage,
        batch: Option<usize>,
    },
    FileComplete {
        file_id: String,
    },
    FilesComplete {
        file_ids: Vec<String>,
    },
    Candidate {
        finding: ReviewFinding,
    },
    Checked {
        finding: ReviewFinding,
    },
    Discard {
        finding_id: String,
        reason: String,
    },
    Finalize {},
}
pub fn authorize_review(
    run: &ReviewRun,
    project_id: Uuid,
    reviewer_id: Uuid,
    run_id: Uuid,
    now: u64,
) -> Result<()> {
    ensure!(
        run.version == REVIEW_VERSION
            && run.id == run_id
            && run.project_id == project_id
            && run.reviewer_id == reviewer_id,
        "Review tool scope does not match the authorized reviewer and run"
    );
    ensure!(
        run.state == ReviewRunState::Running,
        "Review is not accepting tool calls"
    );
    ensure!(now < run.deadline_at, "Review deadline reached");
    Ok(())
}
pub fn validate_input(run: &ReviewRun, input: &ReviewInput) -> Result<()> {
    ensure!(
        input.version == REVIEW_VERSION && input.snapshot_id == run.snapshot_id,
        "Review snapshot identity mismatch"
    );
    Ok(())
}
pub fn review_diff_page(
    run: &mut ReviewRun,
    input: &ReviewInput,
    storage: &Path,
    file_id: &str,
    page: usize,
) -> Result<String> {
    validate_input(run, input)?;
    let f = run
        .files
        .iter_mut()
        .find(|f| f.id == file_id)
        .context("File is outside the assigned review scope")?;
    ensure!(
        f.status != ReviewFileStatus::Skipped,
        "This file was explicitly skipped"
    );
    let diff = input
        .diffs
        .get(file_id)
        .and_then(|pages| pages.get(page))
        .context("Invalid diff page")?;
    let text = read_review_blob(storage, &diff.content_hash)?;
    ensure!(
        text.chars().count() == diff.characters && diff.characters <= DIFF_PAGE_CHARS,
        "Diff page identity changed"
    );
    f.consumed_pages.insert(page);
    if f.status != ReviewFileStatus::Complete {
        f.status = ReviewFileStatus::Reviewing;
    }
    run.consulted_paths.insert(f.path.clone());
    if run.stage == ReviewStage::Understanding {
        run.stage = ReviewStage::Reviewing;
    }
    run.current_group = input
        .batches
        .iter()
        .find(|batch| batch.file_ids.iter().any(|id| id == file_id))
        .map(|batch| batch.group.clone());
    run.revision += 1;
    Ok(text)
}

/// Consume a bounded group of assigned pages in one tool transaction. A bad
/// page must not record consumption of earlier pages in the same request.
pub fn review_batch_page(run: &mut ReviewRun, input: &ReviewInput, storage: &Path,
    batch_id: usize, page: usize) -> Result<serde_json::Value> {
    validate_input(run, input)?;
    let batch = input.batches.iter().find(|b| b.id == batch_id).context("Unknown review batch")?;
    ensure!(!batch.file_ids.is_empty() && batch.file_ids.len() <= 5, "Invalid batch size");
    let mut groups = vec![Vec::new()]; let mut characters = 0;
    for id in &batch.file_ids {
        for (index, diff) in input.diffs.get(id).context("Missing batch diff")?.iter().enumerate() {
            if !groups.last().unwrap().is_empty() && characters + diff.characters > DIFF_PAGE_CHARS {
                groups.push(Vec::new()); characters = 0;
            }
            ensure!(diff.characters <= DIFF_PAGE_CHARS, "Oversized diff page");
            characters += diff.characters; groups.last_mut().unwrap().push((id, index, diff));
        }
    }
    ensure!(page < groups.len() && !groups[page].is_empty(), "Invalid batch page");
    let mut next = run.clone(); let mut pages = Vec::new();
    for (id, index, diff) in &groups[page] {
        let text = review_diff_page(&mut next, input, storage, id, *index)?;
        let file = next.files.iter().find(|f| &f.id == *id).context("Missing batch file")?;
        pages.push(serde_json::json!({"file_id":id,"path":file.path,"page":index,
            "total_pages":file.diff_pages,"content_hash":diff.content_hash,
            "before_hash":file.before_hash,"after_hash":file.after_hash,"text":text}));
    }
    *run = next;
    Ok(serde_json::json!({"batch":batch_id,"group":batch.group,"page":page,
        "total_pages":groups.len(),"diffs":pages}))
}
fn evidence_hash<'a>(
    run: &'a ReviewRun,
    input: &'a ReviewInput,
    path: &Path,
    file_id: Option<&str>,
    side: ReviewSide,
) -> Result<&'a str> {
    if side == ReviewSide::Source {
        return input
            .source
            .get(path)
            .map(String::as_str)
            .context("Source is unavailable or excluded from the snapshot");
    }
    let f = run
        .files
        .iter()
        .find(|f| Some(f.id.as_str()) == file_id && f.path == path)
        .context("Evidence file is outside the assigned scope")?;
    match side {
        ReviewSide::Before => f.before_hash.as_deref(),
        ReviewSide::After => f.after_hash.as_deref(),
        ReviewSide::Source => unreachable!(),
    }
    .context("Evidence contents are unavailable")
}
/// One-based inclusive lines. Exact excerpts are never reconstructed from the
/// live workspace, and cannot refer to a different content identity.
pub fn review_excerpt(text: &str, start: u32, end: u32) -> Result<String> {
    ensure!(
        start > 0 && end >= start && end - start < 200,
        "Invalid or oversized line range"
    );
    let lines: Vec<_> = text.lines().collect();
    ensure!(
        end as usize <= lines.len(),
        "Line range is outside snapshot contents"
    );
    let result = lines[start as usize - 1..end as usize].join("\n");
    ensure!(
        result.len() <= 40_000,
        "Source range is too large; request fewer lines"
    );
    Ok(result)
}

fn patch_only(input: &ReviewInput, file_id: Option<&str>, side: ReviewSide) -> bool {
    side != ReviewSide::Source && file_id.is_some_and(|id| input.patch_only_files.contains(id))
}

fn evidence_excerpt(input: &ReviewInput, file_id: Option<&str>, side: ReviewSide,
    text: &str, start: u32, end: u32) -> Result<String> {
    if !patch_only(input, file_id, side) { return review_excerpt(text, start, end); }
    ensure!(start > 0 && end >= start && end - start < 200, "Invalid or oversized line range");
    let lines: BTreeMap<u32, String> = serde_json::from_str(text)
        .context("Recorded patch line snapshot is invalid")?;
    let excerpt = (start..=end).map(|line| lines.get(&line).cloned()
        .context("Line range includes unrecorded patch lines; narrow it to a recorded hunk or read Source"))
        .collect::<Result<Vec<_>>>()?.join("\n");
    ensure!(excerpt.len() <= 40_000, "Source range is too large; request fewer lines");
    Ok(excerpt)
}
pub fn review_source(
    run: &mut ReviewRun,
    input: &ReviewInput,
    storage: &Path,
    path: &Path,
    file_id: Option<&str>,
    side: ReviewSide,
    start: u32,
    end: u32,
) -> Result<serde_json::Value> {
    validate_input(run, input)?;
    let hash = evidence_hash(run, input, path, file_id, side)?.to_owned();
    let text = read_review_blob(storage, &hash)?;
    let excerpt = evidence_excerpt(input, file_id, side, &text, start, end)?;
    let numbered = excerpt
        .lines()
        .enumerate()
        .map(|(i, l)| format!("{}: {}", start as usize + i, l))
        .collect::<Vec<_>>()
        .join("\n");
    run.consulted_paths.insert(path.to_path_buf());
    run.revision += 1;
    let recorded_patch = patch_only(input, file_id, side);
    let total_lines = if recorded_patch { None } else { Some(text.lines().count()) };
    Ok(
        serde_json::json!({"path":path,"file_id":file_id,"side":side,"content_hash":hash,"start":start,"end":end,"total_lines":total_lines,"recorded_patch_lines":recorded_patch,"text":numbered,"excerpt":excerpt}),
    )
}
pub fn review_search(
    run: &mut ReviewRun,
    input: &ReviewInput,
    storage: &Path,
    query: &str,
    prefix: Option<&Path>,
) -> Result<serde_json::Value> {
    validate_input(run, input)?;
    ensure!(
        !query.trim().is_empty() && query.len() <= 256,
        "Search must be 1–256 characters"
    );
    let mut matches = vec![];
    let mut scanned = 0;
    let mut limited = false;
    for (path, hash) in &input.source {
        if prefix.is_some_and(|prefix| !path.starts_with(prefix)) {
            continue;
        }
        if scanned >= 200 {
            limited = true;
            break;
        }
        scanned += 1;
        let text = read_review_blob(storage, hash)?;
        run.consulted_paths.insert(path.clone());
        for (i, line) in text.lines().enumerate() {
            if line.contains(query) {
                matches.push(serde_json::json!({"path":path,"line":i+1,"content_hash":hash,"text":line.chars().take(500).collect::<String>()}));
                if matches.len() == 100 {
                    limited = true;
                    break;
                }
            }
        }
        if limited {
            break;
        }
    }
    run.revision += 1;
    Ok(
        serde_json::json!({"matches":matches,"scanned_files":scanned,"limited":limited,"note":"Literal bounded search; narrow the prefix when limited"}),
    )
}
pub fn validate_finding(
    run: &ReviewRun,
    input: &ReviewInput,
    storage: &Path,
    finding: &ReviewFinding,
    checked: bool,
) -> Result<()> {
    ensure!(
        !finding.id.is_empty()
            && finding.id.len() <= 100
            && finding
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')),
        "Invalid finding ID"
    );
    for (label, text) in [
        ("title", &finding.title),
        ("trigger", &finding.trigger),
        ("consequence", &finding.consequence),
        ("suggested fix", &finding.suggested_fix),
    ] {
        ensure!(
            !text.trim().is_empty() && text.len() <= 4000,
            "Finding requires a bounded concrete {label}"
        );
    }
    ensure!(
        !checked || (!finding.challenge.trim().is_empty() && finding.challenge.len() <= 8000),
        "Checked finding requires a recorded reviewer challenge"
    );
    let loc = &finding.location;
    ensure!(
        loc.side != ReviewSide::Source
            && loc.start > 0
            && loc.end >= loc.start
            && loc.end - loc.start < 200,
        "Finding must anchor to attributed changed lines"
    );
    let file = run
        .files
        .iter()
        .find(|f| f.id == loc.file_id && f.path == loc.path)
        .context("Finding path is outside scope")?;
    ensure!(
        (loc.start..=loc.end).all(|n| file
            .attributed_ranges
            .iter()
            .any(|r| r.side == loc.side && r.start <= n && r.end >= n)),
        "Finding anchor contains lines not owned by this conversation"
    );
    ensure!(
        !finding.evidence.is_empty() && finding.evidence.len() <= 20,
        "Finding requires bounded snapshot evidence"
    );
    let mut anchor_evidence = false;
    for evidence in &finding.evidence {
        let hash = evidence_hash(
            run,
            input,
            &evidence.path,
            evidence.file_id.as_deref(),
            evidence.side,
        )?;
        ensure!(
            hash == evidence.content_hash,
            "Evidence content identity mismatch"
        );
        let text = read_review_blob(storage, hash)?;
        ensure!(
            evidence_excerpt(input, evidence.file_id.as_deref(), evidence.side,
                &text, evidence.start, evidence.end)? == evidence.excerpt,
            "Evidence excerpt does not match snapshot lines"
        );
        anchor_evidence |= evidence.path == loc.path
            && evidence.file_id.as_deref() == Some(loc.file_id.as_str())
            && evidence.side == loc.side
            && evidence.start <= loc.start
            && evidence.end >= loc.end;
    }
    ensure!(
        anchor_evidence,
        "Finding requires evidence covering its changed-line anchor"
    );
    Ok(())
}
pub fn apply_review_report(
    run: &mut ReviewRun,
    input: &ReviewInput,
    storage: &Path,
    report: ReviewReport,
    now: u64,
) -> Result<()> {
    validate_input(run, input)?;
    ensure!(
        run.state == ReviewRunState::Running && now < run.deadline_at,
        "Review no longer accepts reports"
    );
    match report {
        ReviewReport::Stage { stage, batch } => {
            let ordinal = |s| match s {
                ReviewStage::Understanding => 0,
                ReviewStage::Reviewing => 1,
                ReviewStage::CheckingFindings => 2,
            };
            ensure!(
                ordinal(stage) >= ordinal(run.stage),
                "Review stage cannot go backwards"
            );
            let group = batch
                .map(|id| {
                    input
                        .batches
                        .iter()
                        .find(|b| b.id == id)
                        .map(|b| b.group.clone())
                        .context("Unknown review batch")
                })
                .transpose()?;
            run.stage = stage;
            run.current_group = group;
        }
        ReviewReport::FileComplete { file_id } => {
            let file = run
                .files
                .iter_mut()
                .find(|f| f.id == file_id)
                .context("Unknown review file")?;
            ensure!(
                file.status != ReviewFileStatus::Skipped
                    && file.diff_pages > 0
                    && (0..file.diff_pages).all(|p| file.consumed_pages.contains(&p)),
                "File completion requires consumption of every assigned diff page"
            );
            file.status = ReviewFileStatus::Complete;
        }
        ReviewReport::FilesComplete { file_ids } => {
            ensure!(!file_ids.is_empty() && file_ids.len() <= 5, "Report one to five completed files");
            let ids: BTreeSet<_> = file_ids.iter().collect();
            ensure!(ids.len() == file_ids.len(), "Duplicate completion IDs");
            let mut indices = Vec::new();
            for id in file_ids {
                let index = run.files.iter().position(|f| f.id == id).context("Unknown review file")?;
                let file = &run.files[index];
                ensure!(file.status != ReviewFileStatus::Skipped && file.diff_pages > 0
                    && (0..file.diff_pages).all(|p| file.consumed_pages.contains(&p)),
                    "File completion requires consumption of every assigned diff page");
                indices.push(index);
            }
            for index in indices { run.files[index].status = ReviewFileStatus::Complete; }
        }
        ReviewReport::Candidate { finding } => {
            validate_finding(run, input, storage, &finding, false)?;
            if let Some(previous) = run
                .candidates
                .iter()
                .chain(&run.findings)
                .find(|f| f.id == finding.id)
            {
                ensure!(
                    previous == &finding,
                    "Finding ID already has different contents"
                );
            } else {
                ensure!(
                    run.candidates.len() + run.findings.len() < 200,
                    "Finding limit reached"
                );
                run.candidates.push(finding);
            }
        }
        ReviewReport::Checked { finding } => {
            validate_finding(run, input, storage, &finding, true)?;
            if let Some(previous) = run.findings.iter().find(|f| f.id == finding.id) {
                ensure!(
                    previous == &finding,
                    "Checked finding ID already has different contents"
                );
            } else {
                let index = run
                    .candidates
                    .iter()
                    .position(|f| f.id == finding.id)
                    .context("Submit candidate before checked finding")?;
                let mut previous = run.candidates[index].clone();
                previous.challenge = finding.challenge.clone();
                ensure!(
                    previous == finding,
                    "Checked finding changed its candidate; discard and submit a new candidate"
                );
                ensure!(
                    !run.findings
                        .iter()
                        .any(|f| f.location == finding.location && f.trigger == finding.trigger),
                    "Duplicate finding scenario"
                );
                run.candidates.remove(index);
                run.consulted_paths
                    .extend(finding.evidence.iter().map(|e| e.path.clone()));
                run.findings.push(finding);
            }
        }
        ReviewReport::Discard { finding_id, reason } => {
            ensure!(
                !reason.trim().is_empty() && reason.len() <= 4000,
                "Discard requires reason"
            );
            run.candidates.retain(|f| f.id != finding_id);
        }
        ReviewReport::Finalize {} => {
            ensure!(
                run.stage == ReviewStage::CheckingFindings,
                "Challenge findings before finalizing the review"
            );
            ensure!(
                run.candidates.is_empty(),
                "Resolve pending candidates before finalizing: submit checked with the identical candidate fields plus challenge, or discard with a reason. Use review_context to retrieve pending candidates"
            );
            let complete = run.limitations.is_empty()
                && !run.files.is_empty()
                && run.completed_files() == run.total_files();
            run.finalized_by_reviewer = true;
            run.stop(
                if complete {
                    ReviewRunState::Complete
                } else {
                    ReviewRunState::Partial
                },
                None,
                now,
            );
        }
    }
    run.revision += 1;
    Ok(())
}
