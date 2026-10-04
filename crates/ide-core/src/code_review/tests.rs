use super::*;
use crate::{
    agent_changes::{ChangeKey, EvidenceKind, MutationEvidence},
    local_store::LocalStore,
};
use std::{fs, path::Path, sync::atomic::AtomicBool};

fn requirements() -> ReviewRequirements {
    ReviewRequirements {
        user_requirements: vec!["Preserve concurrent edits".into()],
        decisions: vec![],
        checks: vec![],
        project_rules: vec![],
        supplementary_guidance: String::new(),
    }
}
fn receipt(
    run: &ReviewRun,
    root: &Path,
    path: &str,
    before: &str,
    after: &str,
    action: &str,
) -> MutationEvidence {
    MutationEvidence {
        key: ChangeKey {
            project_id: run.project_id,
            root: root.into(),
            agent_id: run.parent_id,
            generation: "generation".into(),
            turn_id: "turn".into(),
            action_id: action.into(),
            path: path.into(),
        },
        kind: EvidenceKind::Contents,
        confirmed: true,
        additions: None,
        deletions: None,
        before_hash: Some(content_hash(before.as_bytes())),
        after_hash: Some(content_hash(after.as_bytes())),
        before: Some(before.into()),
        after: Some(after.into()),
        patch: None,
        captured_at: 1,
    }
}
struct Fixture {
    _dir: tempfile::TempDir,
    root: std::path::PathBuf,
    storage: std::path::PathBuf,
    run: ReviewRun,
    input: ReviewInput,
}
fn fixture(before: &str, after: &str) -> Fixture {
    fixture_with_patch(before, after, None)
}
fn fixture_with_patch(before: &str, after: &str, patch: Option<crate::git::FileDiff>) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("repo");
    fs::create_dir(&root).unwrap();
    git2::Repository::init(&root).unwrap();
    if !after.is_empty() {
        fs::write(root.join("shared.rs"), after).unwrap();
    }
    let mut run = ReviewRun::new(
        Uuid::new_v4(),
        Uuid::new_v4(),
        "Claude".into(),
        "model".into(),
        "effort".into(),
        review_now(),
    );
    let mut evidence = receipt(&run, &root, "shared.rs", before, after, "edit");
    if let Some(patch) = patch {
        evidence.kind = EvidenceKind::Patch;
        evidence.before = None;
        evidence.after = None;
        evidence.patch = Some(patch);
    }
    let storage = dir.path().join("snapshots");
    let input = prepare_review(
        &mut run,
        &root,
        &storage,
        requirements(),
        &[evidence],
        &[],
        &AtomicBool::new(false),
    )
    .unwrap();
    Fixture {
        _dir: dir,
        root,
        storage,
        run,
        input,
    }
}
fn finding(f: &Fixture, side: ReviewSide, line: u32) -> ReviewFinding {
    let file = &f.run.files[0];
    let hash = match side {
        ReviewSide::Before => file.before_hash.as_ref().unwrap(),
        _ => file.after_hash.as_ref().unwrap(),
    };
    let text = read_review_blob(&f.storage, hash).unwrap();
    ReviewFinding {
        id: "missing-guard".into(),
        severity: ReviewSeverity::High,
        location: ReviewLocation {
            file_id: file.id.clone(),
            path: file.path.clone(),
            side,
            start: line,
            end: line,
        },
        title: "Request can bypass the guard".into(),
        trigger: "Send a stale request while cancellation is pending".into(),
        consequence: "The request changes state after cancellation".into(),
        suggested_fix: "Check run identity and terminal state before applying the request".into(),
        challenge:
            "The caller does not hold the lock; the available guard runs after this mutation".into(),
        evidence: vec![ReviewEvidence {
            path: file.path.clone(),
            file_id: Some(file.id.clone()),
            side,
            content_hash: hash.clone(),
            start: line,
            end: line,
            excerpt: if f.input.patch_only_files.contains(&file.id) {
                serde_json::from_str::<BTreeMap<u32, String>>(&text).unwrap()[&line].clone()
            } else { review_excerpt(&text, line, line).unwrap() },
        }],
    }
}

#[test]
fn patch_receipts_account_for_real_changes_and_support_checked_findings() {
    let before = "first\nold guard\nthird\nfourth\nfifth\nsixth\nseventh\n";
    let after = "first\nnew guard\nthird\nfourth\nfifth\nsixth\nseventh\n";
    let patch = crate::git::diff::diff_from_contents(Path::new("shared.rs"), before, after).unwrap();
    let mut f = fixture_with_patch(before, after, Some(patch));
    let file = f.run.files[0].clone();
    assert_eq!(file.status, ReviewFileStatus::Pending);
    assert!(f.input.patch_only_files.contains(&file.id));
    assert!(f.run.limitations.is_empty(), "{:?}", f.run.limitations);
    let old = review_source(&mut f.run, &f.input, &f.storage, &file.path, Some(&file.id),
        ReviewSide::Before, 2, 2).unwrap();
    assert_eq!(old["excerpt"], "old guard");
    assert_eq!(old["recorded_patch_lines"], true);
    assert!(old["total_lines"].is_null(), "a hunk is not a whole-file line count");
    let checked = finding(&f, ReviewSide::After, 2);
    validate_finding(&f.run, &f.input, &f.storage, &checked, true).unwrap();
    let mut fabricated = checked.clone();
    fabricated.evidence[0].excerpt = "invented guard".into();
    assert!(validate_finding(&f.run, &f.input, &f.storage, &fabricated, true).is_err());
    let mut foreign = checked.clone();
    foreign.location.start = 6; foreign.location.end = 6;
    assert!(validate_finding(&f.run, &f.input, &f.storage, &foreign, true).is_err());
    apply_review_report(&mut f.run, &f.input, &f.storage,
        ReviewReport::Candidate { finding: checked.clone() }, review_now()).unwrap();
    apply_review_report(&mut f.run, &f.input, &f.storage,
        ReviewReport::Checked { finding: checked }, review_now()).unwrap();
    finish(&mut f);
    assert_eq!(f.run.state, ReviewRunState::Complete);
    assert_eq!(f.run.completed_files(), 1);
    assert_eq!(f.run.findings.len(), 1);
    assert_eq!(fs::read_to_string(f.root.join("shared.rs")).unwrap(), after);
}

#[test]
fn patch_reads_reject_unrecorded_gaps_and_keep_surrounding_source_separate() {
    let before = (1..=50).map(|n| format!("line {n}\n")).collect::<String>();
    let after = before.replace("line 10\n", "changed ten\n").replace("line 30\n", "changed thirty\n");
    let patch = crate::git::diff::diff_from_contents(Path::new("shared.rs"), &before, &after).unwrap();
    let mut f = fixture_with_patch(&before, &after, Some(patch));
    let file = f.run.files[0].clone();
    assert!(review_source(&mut f.run, &f.input, &f.storage, &file.path, Some(&file.id),
        ReviewSide::Before, 20, 20).unwrap_err().to_string().contains("unrecorded"));
    // A later live edit cannot rewrite either frozen evidence view.
    fs::write(f.root.join("shared.rs"), "another writer changed the live file\n").unwrap();
    let current = review_source(&mut f.run, &f.input, &f.storage, &file.path, None,
        ReviewSide::Source, 20, 20).unwrap();
    assert_eq!(current["excerpt"], "line 20");
    assert_eq!(current["recorded_patch_lines"], false);
    assert_eq!(current["total_lines"], 50);
    let changed = review_source(&mut f.run, &f.input, &f.storage, &file.path, Some(&file.id),
        ReviewSide::After, 10, 10).unwrap();
    assert_eq!(changed["excerpt"], "changed ten");
}

#[test]
fn invalid_patch_paths_binary_and_conflicting_lines_remain_explicitly_skipped() {
    let patch = crate::git::diff::diff_from_contents(Path::new("shared.rs"), "old\n", "new\n").unwrap();
    for invalid in 0..3 {
        let mut patch = patch.clone();
        match invalid {
            0 => patch.path = "unrelated.rs".into(),
            1 => patch.is_binary = true,
            _ => {
                let mut conflicting = patch.hunks[0].lines.iter()
                    .find(|line| line.origin == crate::git::LineOrigin::Add).unwrap().clone();
                conflicting.text = "fabricated replacement\n".into();
                patch.hunks[0].lines.push(conflicting);
            }
        }
        let f = fixture_with_patch("old\n", "new\n", Some(patch));
        assert_eq!(f.run.files[0].status, ReviewFileStatus::Skipped);
        assert!(f.run.files[0].skip_reason.as_ref().unwrap().contains("Invalid recorded patch"));
        assert!(f.input.diffs.is_empty());
        assert!(!f.run.is_clean());
    }
}

#[test]
fn recorded_patch_crlf_lines_match_the_frozen_source_excerpt() {
    let before = "first\r\nold guard\r\nlast without newline";
    let after = "first\r\nnew guard\r\nlast without newline";
    let patch = crate::git::diff::diff_from_contents(Path::new("shared.rs"), before, after).unwrap();
    let mut f = fixture_with_patch(before, after, Some(patch));
    let file = f.run.files[0].clone();
    let recorded = review_source(&mut f.run, &f.input, &f.storage, &file.path, Some(&file.id),
        ReviewSide::After, 1, 3).unwrap();
    let source = review_source(&mut f.run, &f.input, &f.storage, &file.path, None,
        ReviewSide::Source, 1, 3).unwrap();
    assert_eq!(recorded["excerpt"], source["excerpt"]);
    validate_finding(&f.run, &f.input, &f.storage, &finding(&f, ReviewSide::After, 2), true).unwrap();
}

#[test]
fn zero_context_patches_do_not_claim_unknown_contents_are_an_empty_file() {
    let before = "one\ntwo\nthree\n";
    let after = "one\ntwo\nnew\nthree\n";
    let mut patch = crate::git::diff::diff_from_contents(Path::new("shared.rs"), before, after).unwrap();
    for hunk in &mut patch.hunks {
        hunk.lines.retain(|line| line.origin == crate::git::LineOrigin::Add);
    }
    let mut f = fixture_with_patch(before, after, Some(patch));
    let file = f.run.files[0].clone();
    assert_eq!(file.change_kind, "Recorded patch");
    assert!(review_source(&mut f.run, &f.input, &f.storage, &file.path, Some(&file.id),
        ReviewSide::Before, 1, 1).is_err());
    let read = review_source(&mut f.run, &f.input, &f.storage, &file.path, Some(&file.id),
        ReviewSide::After, 3, 3).unwrap();
    assert_eq!(read["excerpt"], "new");
    assert_eq!(read["total_lines"], serde_json::Value::Null);
}

#[test]
fn legacy_full_content_snapshots_need_no_patch_metadata() {
    let f = fixture("old\n", "new\n");
    let mut raw = serde_json::to_value(&f.input).unwrap();
    raw.as_object_mut().unwrap().remove("patch_only_files");
    let input: ReviewInput = serde_json::from_value(raw).unwrap();
    assert!(input.patch_only_files.is_empty());
    validate_finding(&f.run, &input, &f.storage, &finding(&f, ReviewSide::After, 1), true).unwrap();
}
fn finish(f: &mut Fixture) {
    for file in f
        .run
        .files
        .clone()
        .into_iter()
        .filter(|f| f.status != ReviewFileStatus::Skipped)
    {
        for page in 0..file.diff_pages {
            review_diff_page(&mut f.run, &f.input, &f.storage, &file.id, page).unwrap();
        }
        apply_review_report(
            &mut f.run,
            &f.input,
            &f.storage,
            ReviewReport::FileComplete { file_id: file.id },
            review_now(),
        )
        .unwrap();
    }
    apply_review_report(
        &mut f.run,
        &f.input,
        &f.storage,
        ReviewReport::Stage {
            stage: ReviewStage::CheckingFindings,
            batch: None,
        },
        review_now(),
    )
    .unwrap();
    apply_review_report(
        &mut f.run,
        &f.input,
        &f.storage,
        ReviewReport::Finalize {},
        review_now(),
    )
    .unwrap();
}

#[test]
fn premature_finalization_keeps_candidates_recoverable_until_checked_or_discarded() {
    for checked in [false, true] {
        let mut f = fixture("old\n", "new\n");
        let candidate = finding(&f, ReviewSide::After, 1);
        apply_review_report(&mut f.run, &f.input, &f.storage,
            ReviewReport::Candidate { finding:candidate.clone() }, review_now()).unwrap();
        f.run.stage = ReviewStage::CheckingFindings;
        let before = f.run.clone();
        let error = apply_review_report(&mut f.run, &f.input, &f.storage,
            ReviewReport::Finalize {}, review_now()).unwrap_err();
        assert!(error.to_string().contains("Resolve pending candidates"));
        assert_eq!(f.run, before);
        assert!(!f.run.finalized_by_reviewer);
        let report = if checked { ReviewReport::Checked { finding:candidate } }
            else { ReviewReport::Discard { finding_id:candidate.id, reason:"Caller already prevents the trigger".into() } };
        apply_review_report(&mut f.run, &f.input, &f.storage, report, review_now()).unwrap();
        finish(&mut f);
        assert_eq!(f.run.state, ReviewRunState::Complete);
        assert_eq!(f.run.findings.len(), usize::from(checked));
    }
}

#[test]
fn only_attributed_lines_can_anchor_a_checked_finding() {
    let mut f = fixture("other writer\nold\n", "other writer\nnew\n");
    assert!(validate_finding(
        &f.run,
        &f.input,
        &f.storage,
        &finding(&f, ReviewSide::After, 1),
        true
    )
    .is_err());
    let valid = finding(&f, ReviewSide::After, 2);
    assert!(apply_review_report(
        &mut f.run,
        &f.input,
        &f.storage,
        ReviewReport::Checked {
            finding: valid.clone()
        },
        review_now()
    )
    .is_err());
    apply_review_report(
        &mut f.run,
        &f.input,
        &f.storage,
        ReviewReport::Candidate {
            finding: valid.clone(),
        },
        review_now(),
    )
    .unwrap();
    assert!(f.run.findings.is_empty());
    apply_review_report(
        &mut f.run,
        &f.input,
        &f.storage,
        ReviewReport::Checked {
            finding: valid.clone(),
        },
        review_now(),
    )
    .unwrap();
    apply_review_report(
        &mut f.run,
        &f.input,
        &f.storage,
        ReviewReport::Checked { finding: valid },
        review_now(),
    )
    .unwrap();
    assert_eq!(f.run.findings.len(), 1);
    let mut forged = finding(&f, ReviewSide::After, 2);
    forged.evidence[0].excerpt = "fabricated guard".into();
    assert!(validate_finding(&f.run, &f.input, &f.storage, &forged, true).is_err());
    let mut unchallenged = finding(&f, ReviewSide::After, 2);
    unchallenged.challenge.clear();
    assert!(validate_finding(&f.run, &f.input, &f.storage, &unchallenged, true).is_err());
}
#[test]
fn deletions_have_before_side_evidence_without_reading_live_source() {
    let f = fixture("important guard\n", "");
    validate_finding(
        &f.run,
        &f.input,
        &f.storage,
        &finding(&f, ReviewSide::Before, 1),
        true,
    )
    .unwrap();
    assert!(f.input.source.get(Path::new("shared.rs")).is_none());
    assert!(!f.root.join("shared.rs").exists());
}
#[test]
fn diff_pages_not_names_account_for_completion_and_utf8_is_preserved() {
    let mut f = fixture("old\n", &format!("{}\n", "שלום".repeat(30_000)));
    let file = f.run.files[0].clone();
    assert!(file.diff_pages > 1);
    let report = || ReviewReport::FileComplete {
        file_id: file.id.clone(),
    };
    assert!(apply_review_report(&mut f.run, &f.input, &f.storage, report(), review_now()).is_err());
    review_diff_page(&mut f.run, &f.input, &f.storage, &file.id, 0).unwrap();
    assert_eq!(f.run.stage, ReviewStage::Reviewing);
    assert_eq!(f.run.current_group, Some(f.input.batches[0].group.clone()));
    assert!(apply_review_report(&mut f.run, &f.input, &f.storage, report(), review_now()).is_err());
    for page in 1..file.diff_pages {
        let text = review_diff_page(&mut f.run, &f.input, &f.storage, &file.id, page).unwrap();
        assert!(text.chars().count() <= DIFF_PAGE_CHARS);
    }
    apply_review_report(&mut f.run, &f.input, &f.storage, report(), review_now()).unwrap();
    apply_review_report(&mut f.run, &f.input, &f.storage, report(), review_now()).unwrap();
    assert_eq!(f.run.completed_files(), 1);
    assert!(review_diff_page(&mut f.run, &f.input, &f.storage, &file.id, file.diff_pages).is_err());
}

#[test]
fn a_deleted_file_recreated_by_another_writer_cannot_receive_clean_coverage() {
    let mut f = fixture("important guard\n", "replacement from another writer\n");
    f.run.files.clear(); f.run.state = ReviewRunState::Preparing;
    let deletion = receipt(&f.run,&f.root,"shared.rs","important guard\n","","delete");
    f.input = prepare_review(&mut f.run,&f.root,&f.storage,requirements(),&[deletion],&[],&AtomicBool::new(false)).unwrap();
    assert_eq!(read_review_blob(&f.storage,&f.input.source[Path::new("shared.rs")]).unwrap(),"replacement from another writer\n");
    assert!(f.run.limitations.iter().any(|reason| reason.contains("subsequent or unavailable source")));
    let id = f.run.files[0].id.clone();
    review_diff_page(&mut f.run,&f.input,&f.storage,&id,0).unwrap();
    apply_review_report(&mut f.run,&f.input,&f.storage,ReviewReport::FileComplete{file_id:id},review_now()).unwrap();
    apply_review_report(&mut f.run,&f.input,&f.storage,ReviewReport::Stage{stage:ReviewStage::CheckingFindings,batch:None},review_now()).unwrap();
    apply_review_report(&mut f.run,&f.input,&f.storage,ReviewReport::Finalize{},review_now()).unwrap();
    assert_eq!(f.run.state,ReviewRunState::Partial);
    assert!(!f.run.is_clean());
}
#[test]
fn clean_requires_complete_current_unlimited_coverage() {
    let mut f = fixture("old\n", "new\n");
    finish(&mut f);
    assert!(f.run.is_clean());
    for state in [
        ReviewRunState::Preparing,
        ReviewRunState::Running,
        ReviewRunState::Cancelling,
        ReviewRunState::Partial,
        ReviewRunState::Cancelled,
        ReviewRunState::Failed,
        ReviewRunState::Interrupted,
    ] {
        let mut run = f.run.clone();
        run.state = state;
        assert!(!run.is_clean());
    }
    f.run.freshness = ReviewFreshness::Uncertain("Missing source".into());
    assert!(!f.run.is_clean());
    assert!(!f.run.can_fix());
    f.run.freshness = ReviewFreshness::Outdated("Changed".into());
    assert!(!f.run.can_fix());
}
#[test]
fn partial_unchecked_and_deadline_results_cannot_be_clean() {
    let mut f = fixture("old\n", "new\n");
    let bug = finding(&f, ReviewSide::After, 1);
    apply_review_report(
        &mut f.run,
        &f.input,
        &f.storage,
        ReviewReport::Candidate { finding: bug },
        review_now(),
    )
    .unwrap();
    f.run.stop(ReviewRunState::Partial, Some("Deadline reached before challenging candidates".into()),
        f.run.deadline_at);
    assert_eq!(f.run.state, ReviewRunState::Partial);
    assert!(f.run.findings.is_empty());
    assert!(!f.run.is_clean());
    let f = fixture("old\n", "new\n");
    assert!(authorize_review(
        &f.run,
        f.run.project_id,
        f.run.reviewer_id,
        f.run.id,
        f.run.deadline_at
    )
    .is_err());
}
#[test]
fn read_search_and_freshness_use_snapshot_not_the_live_workspace() {
    let mut f = fixture("old\n", "new\n");
    fs::write(f.root.join("shared.rs"), "later mutation\n").unwrap();
    let read = review_source(
        &mut f.run,
        &f.input,
        &f.storage,
        Path::new("shared.rs"),
        None,
        ReviewSide::Source,
        1,
        1,
    )
    .unwrap();
    assert_eq!(read["excerpt"], "new");
    assert_eq!(
        review_search(&mut f.run, &f.input, &f.storage, "new", None).unwrap()["matches"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(review_source(
        &mut f.run,
        &f.input,
        &f.storage,
        Path::new("../outside"),
        None,
        ReviewSide::Source,
        1,
        1
    )
    .is_err());
    revalidate_review(&mut f.run, &f.input, 0);
    assert!(matches!(f.run.freshness, ReviewFreshness::Outdated(_)));
    let mut f = fixture("old\n", "new\n");
    revalidate_review(&mut f.run, &f.input, 1);
    assert!(matches!(f.run.freshness, ReviewFreshness::Outdated(_)));
}
#[test]
fn provider_reviewer_run_and_snapshot_identity_all_bind_the_tools() {
    let f = fixture("old\n", "new\n");
    authorize_review(
        &f.run,
        f.run.project_id,
        f.run.reviewer_id,
        f.run.id,
        review_now(),
    )
    .unwrap();
    for (project, reviewer, run) in [
        (Uuid::new_v4(), f.run.reviewer_id, f.run.id),
        (f.run.project_id, f.run.parent_id, f.run.id),
        (f.run.project_id, f.run.reviewer_id, Uuid::new_v4()),
    ] {
        assert!(authorize_review(&f.run, project, reviewer, run, review_now()).is_err());
    }
    let mut input = f.input.clone();
    input.snapshot_id = Uuid::new_v4();
    assert!(validate_input(&f.run, &input).is_err());
}
#[test]
fn credentials_artifacts_binary_large_missing_and_unsafe_paths_are_accounted() {
    let mut f = fixture("old\n", "new\n");
    let mut run = ReviewRun::new(
        f.run.project_id,
        f.run.parent_id,
        "Claude".into(),
        "model".into(),
        "effort".into(),
        review_now(),
    );
    let mut evidence = vec![];
    for path in [
        ".env",
        ".git/config",
        "credentials.json",
        "target/generated.rs",
        "../escape",
    ] {
        evidence.push(receipt(
            &run,
            &f.root,
            path,
            "secret",
            "credential",
            "unsafe",
        ));
    }
    evidence.push(receipt(
        &run,
        &f.root,
        "binary.dat",
        "",
        "\0binary",
        "binary",
    ));
    evidence.push(receipt(
        &run,
        &f.root,
        "large.rs",
        "",
        &"x".repeat(MAX_SOURCE_BYTES + 1),
        "large",
    ));
    let mut missing = receipt(&run, &f.root, "unavailable.rs", "old", "new", "missing");
    missing.before = None;
    evidence.push(missing);
    fs::write(f.root.join(".env.example"), "KEY=example").unwrap();
    fs::write(f.root.join(".env"), "KEY=private").unwrap();
    let input = prepare_review(
        &mut run,
        &f.root,
        &f.storage,
        requirements(),
        &evidence,
        &[],
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(run.files.len(), 8);
    assert!(run
        .files
        .iter()
        .all(|f| f.status == ReviewFileStatus::Skipped && f.skip_reason.is_some()));
    assert!(input.source.contains_key(Path::new(".env.example")));
    assert!(!input.source.contains_key(Path::new(".env")));
    assert!(input.diffs.is_empty());
    // Excluded/skipped paths are limitations, never freshness baselines; do
    // not open credentials during revalidation or claim they changed.
    let revision = run.mutation_revision;
    revalidate_review(&mut run, &input, revision);
    assert_eq!(run.freshness, ReviewFreshness::Current);
    f.run = run;
    f.input = input;
    finish(&mut f);
    assert_eq!(f.run.state, ReviewRunState::Partial);
}
#[cfg(unix)]
#[test]
fn parent_and_leaf_symlinks_cannot_expose_outside_source() {
    use std::os::unix::fs::symlink;
    let f = fixture("old\n", "new\n");
    let outside = f._dir.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("secret.rs"), "outside secret").unwrap();
    symlink(&outside, f.root.join("linked")).unwrap();
    symlink(outside.join("secret.rs"), f.root.join("leaf.rs")).unwrap();
    let mut run = ReviewRun::new(
        f.run.project_id,
        f.run.parent_id,
        "Claude".into(),
        "model".into(),
        "effort".into(),
        review_now(),
    );
    let evidence = [
        receipt(&run, &f.root, "linked/secret.rs", "old", "new", "parent"),
        receipt(&run, &f.root, "leaf.rs", "old", "new", "leaf"),
    ];
    let input = prepare_review(
        &mut run,
        &f.root,
        &f.storage,
        requirements(),
        &evidence,
        &[],
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(run
        .files
        .iter()
        .all(|f| f.status == ReviewFileStatus::Skipped));
    assert!(!input.source.contains_key(Path::new("linked/secret.rs")));
    assert!(!input.source.contains_key(Path::new("leaf.rs")));
}
#[test]
fn repeated_patches_review_current_net_edits_and_preserve_unrelated_lines() {
    let original = (1..=30).map(|n| format!("line{n}\n")).collect::<String>();
    let middle = original.replace("line1\n", "own first\n");
    let after = middle.replace("line8\n", "own second\n");
    let current = after.replace("line29\n", "other writer\n");
    let mut f = fixture(&original, &current);
    let mut run = ReviewRun::new(f.run.project_id, f.run.parent_id, "Codex".into(), "model".into(), "High".into(), review_now());
    let mut evidence = Vec::new();
    for (index, (before, after)) in [(&original, &middle), (&middle, &after)].into_iter().enumerate() {
        let mut e = receipt(&run, &f.root, "shared.rs", before, after, &format!("edit{index}"));
        e.kind = EvidenceKind::Patch; e.patch = Some(crate::git::diff::diff_from_contents(&e.key.path, before, after).unwrap());
        e.before = None; e.after = None; e.before_hash = None; e.after_hash = None; e.captured_at = index as u64;
        evidence.push(e);
    }
    f.input = prepare_review(&mut run, &f.root, &f.storage, requirements(), &evidence, &[], &AtomicBool::new(false)).unwrap();
    f.run = run;
    assert_eq!(f.run.files.len(), 1, "historical patches become one provable net diff");
    let before = read_review_blob(&f.storage, f.run.files[0].before_hash.as_deref().unwrap()).unwrap();
    assert_eq!(before, original.replace("line29\n", "other writer\n"));
    assert!(f.run.limitations.is_empty(), "{:?}", f.run.limitations);
    assert!(validate_finding(&f.run, &f.input, &f.storage, &finding(&f, ReviewSide::After, 29), true).is_err());
    finish(&mut f);
    assert_eq!(f.run.state, ReviewRunState::Complete);
    assert_eq!(fs::read_to_string(f.root.join("shared.rs")).unwrap(), current);
}

#[test]
fn replay_refuses_foreign_changes_inside_a_recorded_hunk() {
    let mut f = fixture("old\nguard\n", "foreign\nnew guard\n");
    let mut run = ReviewRun::new(f.run.project_id, f.run.parent_id, "Codex".into(), "model".into(), "High".into(), review_now());
    let mut evidence = Vec::new();
    for (index, (before, after)) in [("old\nguard\n", "own\nguard\n"), ("foreign\nguard\n", "foreign\nnew guard\n")].into_iter().enumerate() {
        let mut e = receipt(&run, &f.root, "shared.rs", before, after, &format!("edit{index}"));
        e.patch = Some(crate::git::diff::diff_from_contents(&e.key.path, before, after).unwrap());
        e.kind = EvidenceKind::Patch; e.before = None; e.after = None; e.before_hash = None; e.after_hash = None; e.captured_at = index as u64;
        evidence.push(e);
    }
    f.input = prepare_review(&mut run, &f.root, &f.storage, requirements(), &evidence, &[], &AtomicBool::new(false)).unwrap(); f.run = run;
    assert_eq!(f.run.files.len(), 2);
    assert!(!f.run.limitations.is_empty());
    let second = f.run.files.iter().position(|file| file.attributed_ranges.iter().any(|r| r.side == ReviewSide::After && r.start == 2)).unwrap();
    f.run.files.swap(0, second);
    assert!(validate_finding(&f.run, &f.input, &f.storage, &finding(&f, ReviewSide::After, 1), true).is_err(), "the foreign replacement is not owned by the second edit");
}

#[test]
fn replay_compacts_the_verified_tail_without_hiding_an_earlier_gap() {
    let mut f = fixture("old\nguard\n", "foreign\nfinal\n");
    let versions = [("old\nguard\n", "own\nguard\n"),
        ("foreign\nguard\n", "foreign\nintermediate\n"),
        ("foreign\nintermediate\n", "foreign\nfinal\n")];
    let receipts: Vec<_> = versions.iter().enumerate().map(|(index, (before, after))| {
        let mut e = receipt(&f.run, &f.root, "shared.rs", before, after, &format!("patch-{index}"));
        e.kind = EvidenceKind::Patch; e.before = None; e.after = None; e.before_hash = None; e.after_hash = None;
        e.patch = Some(crate::git::diff::diff_from_contents(&e.key.path, before, after).unwrap());
        e.captured_at = index as u64; e
    }).collect();
    f.run.files.clear(); f.run.state = ReviewRunState::Preparing;
    f.input = prepare_review(&mut f.run, &f.root, &f.storage, requirements(), &receipts, &[], &AtomicBool::new(false)).unwrap();
    assert_eq!(f.run.files.len(), 2, "keep the historical receipt plus one verified current diff");
    assert_eq!(f.input.patch_only_files.len(), 1);
    assert!(!f.run.limitations.is_empty(), "the missing intervening edit stays explicit");
    let net = f.run.files.iter().find(|file| !f.input.patch_only_files.contains(&file.id)).unwrap();
    assert_eq!(read_review_blob(&f.storage, net.before_hash.as_ref().unwrap()).unwrap(), "foreign\nguard\n");
    assert!(!net.attributed_ranges.iter().any(|r| r.start == 1));
}

#[test]
fn reverted_edits_have_an_empty_net_diff_without_historical_bug_anchors() {
    let mut f = fixture("old\n", "old\n");
    let mut run = ReviewRun::new(f.run.project_id, f.run.parent_id, "Codex".into(), "model".into(), "High".into(), review_now());
    let mut evidence = Vec::new();
    for (index, (before, after)) in [("old\n", "temporary bug\n"), ("temporary bug\n", "old\n")].into_iter().enumerate() {
        let mut e = receipt(&run, &f.root, "shared.rs", before, after, &format!("edit{index}"));
        e.patch = Some(crate::git::diff::diff_from_contents(&e.key.path, before, after).unwrap());
        e.before = None; e.after = None; e.before_hash = None; e.after_hash = None; e.captured_at = index as u64;
        evidence.push(e);
    }
    f.input = prepare_review(&mut run, &f.root, &f.storage, requirements(), &evidence, &[], &AtomicBool::new(false)).unwrap(); f.run = run;
    assert_eq!(f.run.files.len(), 1);
    assert_eq!(f.run.files[0].change_kind, "Unchanged");
    assert!(f.run.files[0].attributed_ranges.is_empty());
    finish(&mut f);
    assert!(f.run.is_clean());
}

#[test]
fn contiguous_own_receipts_collapse_but_shared_hunk_gaps_do_not() {
    let f = fixture("old\n", "new\n");
    let mut run = ReviewRun::new(
        f.run.project_id,
        f.run.parent_id,
        "Claude".into(),
        "model".into(),
        "effort".into(),
        review_now(),
    );
    let evidence = [
        receipt(&run, &f.root, "shared.rs", "old\n", "middle\n", "one"),
        receipt(&run, &f.root, "shared.rs", "middle\n", "new\n", "two"),
    ];
    prepare_review(
        &mut run,
        &f.root,
        &f.storage,
        requirements(),
        &evidence,
        &[],
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(run.files.len(), 1);
    assert!(run.limitations.is_empty());
    let mut run = ReviewRun::new(
        f.run.project_id,
        f.run.parent_id,
        "Claude".into(),
        "model".into(),
        "effort".into(),
        review_now(),
    );
    let evidence = [
        receipt(&run, &f.root, "shared.rs", "old\n", "middle\n", "one"),
        receipt(&run, &f.root, "shared.rs", "other writer\n", "new\n", "two"),
    ];
    prepare_review(
        &mut run,
        &f.root,
        &f.storage,
        requirements(),
        &evidence,
        &[],
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(run.files.len(), 2);
    assert_eq!(run.total_files(), 1);
    assert!(!run.limitations.is_empty());
}
#[test]
fn committed_owned_changes_stay_in_scope_and_unrelated_dirty_files_do_not() {
    let f = fixture("old\n", "new\n");
    let repo = git2::Repository::open(&f.root).unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("shared.rs")).unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let sig = git2::Signature::now("test", "test@example.com").unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, "attributed work", &tree, &[])
        .unwrap();
    fs::write(f.root.join("unrelated.rs"), "other user's dirty file\n").unwrap();
    let mut run = ReviewRun::new(
        f.run.project_id,
        f.run.parent_id,
        "Claude".into(),
        "model".into(),
        "effort".into(),
        review_now(),
    );
    let evidence = receipt(&run, &f.root, "shared.rs", "old\n", "new\n", "edit");
    let input = prepare_review(
        &mut run,
        &f.root,
        &f.storage,
        requirements(),
        &[evidence],
        &[],
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(run.files.len(), 1);
    assert_eq!(run.files[0].path, Path::new("shared.rs"));
    assert!(input.source.contains_key(Path::new("unrelated.rs")));
}
#[test]
fn preparation_cancel_timeout_and_in_repository_storage_stop_without_mutating_source() {
    let f = fixture("old\n", "new\n");
    let mut run = ReviewRun::new(
        f.run.project_id,
        f.run.parent_id,
        "Claude".into(),
        "model".into(),
        "effort".into(),
        review_now(),
    );
    assert!(prepare_review(
        &mut run,
        &f.root,
        &f.storage,
        requirements(),
        &[],
        &[],
        &AtomicBool::new(true)
    )
    .is_err());
    run.deadline_at = review_now();
    assert!(prepare_review(
        &mut run,
        &f.root,
        &f.storage,
        requirements(),
        &[],
        &[],
        &AtomicBool::new(false)
    )
    .is_err());
    run.deadline_at = review_now() + 300;
    assert!(prepare_review(
        &mut run,
        &f.root,
        &f.root,
        requirements(),
        &[],
        &[],
        &AtomicBool::new(false)
    )
    .is_err());
    assert_eq!(
        fs::read_to_string(f.root.join("shared.rs")).unwrap(),
        "new\n"
    );
}
#[test]
fn persistence_serializes_cancel_and_reports_and_recovers_without_restarting() {
    let f = fixture("old\n", "new\n");
    let store = LocalStore::open(f._dir.path().join("store")).unwrap();
    let mut initial = f.run.clone();
    initial.state = ReviewRunState::Preparing;
    initial.revision = 0;
    store.create_review_run(&initial).unwrap();
    assert!(store.create_review_run(&initial).is_err());
    let mut duplicate = initial.clone();
    duplicate.id = Uuid::new_v4();
    assert!(store.create_review_run(&duplicate).is_err());
    store
        .transact_review(initial.id, Some(0), |run| {
            run.state = ReviewRunState::Running;
            Ok(())
        })
        .unwrap();
    assert!(store
        .transact_review(initial.id, Some(0), |_| Ok(()))
        .is_err());
    store
        .transact_review(initial.id, None, |run| {
            run.state = ReviewRunState::Cancelling;
            Ok(())
        })
        .unwrap();
    assert!(store
        .transact_review(initial.id, None, |run| apply_review_report(
            run,
            &f.input,
            &f.storage,
            ReviewReport::Stage {
                stage: ReviewStage::Reviewing,
                batch: None
            },
            review_now()
        ))
        .is_err());
    let recovered = store
        .interrupt_unfinished_reviews(initial.parent_id)
        .unwrap();
    assert_eq!(recovered[0].state, ReviewRunState::Interrupted);
    assert!(!recovered[0].is_clean());
    assert!(store
        .interrupt_unfinished_reviews(initial.parent_id)
        .unwrap()
        .is_empty());
    store.create_review_run(&duplicate).unwrap();
}
#[test]
fn incomplete_receipts_never_establish_complete_ownership() {
    let f = fixture("old\n", "new\n");
    let store = LocalStore::open(f._dir.path().join("store")).unwrap();
    let e = receipt(&f.run, &f.root, "shared.rs", "old\n", "new\n", "edit");
    store.save_mutation_evidence(&e).unwrap();
    let (owned, limitations, revision) = store.review_ownership(f.run.parent_id).unwrap();
    assert_eq!(owned.len(), 1);
    assert!(!limitations.is_empty());
    assert!(revision > 0);
    store
        .save_change_receipt(&crate::agent_changes::ChangeReceipt {
            agent_id: f.run.parent_id,
            generation: "generation".into(),
            turn_id: "turn".into(),
            state: crate::agent_changes::ChangeReceiptState::Ready,
            updated_at: 1,
        })
        .unwrap();
    assert!(store
        .review_ownership(f.run.parent_id)
        .unwrap()
        .1
        .is_empty());
}

#[test]
fn missing_change_contents_do_not_discard_reviewed_files_or_checked_findings() {
    let mut f = fixture("old guard\n", "new behavior\n");
    f.run.files.clear(); f.run.state = ReviewRunState::Preparing;
    let good = receipt(&f.run,&f.root,"shared.rs","old guard\n","new behavior\n","good");
    let mut missing = receipt(&f.run,&f.root,"unavailable.rs","","","missing");
    missing.before = None; missing.after = None; missing.before_hash = None; missing.after_hash = None;
    f.input = prepare_review(&mut f.run,&f.root,&f.storage,requirements(),&[good,missing],&[],&AtomicBool::new(false)).unwrap();
    assert_eq!(f.run.files[1].status,ReviewFileStatus::Skipped);
    let id = f.run.files[0].id.clone();
    review_diff_page(&mut f.run,&f.input,&f.storage,&id,0).unwrap();
    let confirmed = finding(&f,ReviewSide::After,1);
    apply_review_report(&mut f.run,&f.input,&f.storage,ReviewReport::Candidate{finding:confirmed.clone()},review_now()).unwrap();
    apply_review_report(&mut f.run,&f.input,&f.storage,ReviewReport::Checked{finding:confirmed.clone()},review_now()).unwrap();
    apply_review_report(&mut f.run,&f.input,&f.storage,ReviewReport::FileComplete{file_id:id},review_now()).unwrap();
    apply_review_report(&mut f.run,&f.input,&f.storage,ReviewReport::Stage{stage:ReviewStage::CheckingFindings,batch:None},review_now()).unwrap();
    apply_review_report(&mut f.run,&f.input,&f.storage,ReviewReport::Finalize{},review_now()).unwrap();
    assert!(f.run.finalized_by_reviewer);
    assert_eq!(f.run.state,ReviewRunState::Partial);
    assert_eq!(f.run.completed_files(),1);
    assert_eq!(f.run.findings,vec![confirmed]);
    assert!(f.run.can_fix());
    assert!(!f.run.is_clean());
}
#[test]
fn malformed_report_and_unknown_finding_fields_are_rejected() {
    assert!(serde_json::from_value::<ReviewReport>(
        serde_json::json!({"action":"finalize","override_permissions":true})
    )
    .is_err());
    let f = fixture("old\n", "new\n");
    let mut value = serde_json::to_value(finding(&f, ReviewSide::After, 1)).unwrap();
    value["command"] = serde_json::json!("run shell");
    assert!(serde_json::from_value::<ReviewFinding>(value).is_err());
}

#[test]
fn compact_context_preserves_requirements_and_lossless_archived_decisions_and_checks() {
    let mut f = fixture("old\n", "new\n");
    f.input.requirements.user_requirements = vec!["Keep the original behavior".into(), "Correction: allow cancelling".into()];
    f.input.requirements.decisions = (0..22).map(|i| format!("Plan {i}: {}", "x".repeat(3000))).collect();
    f.input.requirements.checks = (0..211).map(|i| ReviewCheck {
        description: serde_json::json!({"command":format!("rg -n 'cargo test' file{i}"),"provider_status":"Completed"}).to_string(),
        provenance:"Provider event".into(),limitation:Some("No pass guarantee".into()) }).collect();
    for status in ["Failed","Completed"] {
        f.input.requirements.checks.push(ReviewCheck { description:serde_json::json!({"command":"/bin/zsh -lc 'RUST_TEST_THREADS=2 cargo test --locked -p ide-core'","provider_status":status}).to_string(),
            provenance:"Provider event".into(),limitation:Some("Provider-reported".into()) });
    }
    let overview = review_context(&f.run, &f.input, "overview", 0).unwrap();
    assert_eq!(overview["requirements"]["user_requirements"], serde_json::json!(f.input.requirements.user_requirements));
    assert!(overview["files"][0].get("attributed_ranges").is_none());
    let previews = overview["checks"]["recent_verification_previews"].as_array().unwrap();
    assert_eq!(previews.len(),1, "search commands are not verification; repeated checks use the latest event");
    assert!(previews[0]["preview"].as_str().unwrap().contains("Completed"));
    assert!(serde_json::to_string(&overview).unwrap().len() < 10_000);
    for (section, expected) in [("decisions",serde_json::json!(f.input.requirements.decisions)),
        ("checks",serde_json::json!(f.input.requirements.checks))] {
        let first = review_context(&f.run, &f.input, section, 0).unwrap();
        let pages = first["total_pages"].as_u64().unwrap(); let mut archived = Vec::new();
        for page in 0..pages { archived.extend(review_context(&f.run,&f.input,section,page as usize).unwrap()["entries"].as_array().unwrap().clone()); }
        assert_eq!(serde_json::json!(archived),expected);
        assert!(review_context(&f.run,&f.input,section,pages as usize).is_err());
    }
    assert!(review_context(&f.run,&f.input,"foreign",0).is_err());
    assert!(review_context(&f.run,&f.input,"overview",1).is_err());
    let details = review_context(&f.run,&f.input,"files",0).unwrap();
    assert_eq!(details["entries"][0]["attributed_ranges"],serde_json::json!(f.run.files[0].attributed_ranges));
    f.input.omitted_source.insert(".env".into(),"Excluded sensitive path".into());
    let omitted = review_context(&f.run,&f.input,"omissions",0).unwrap();
    assert_eq!(omitted["entries"],serde_json::json!([{"path":".env","reason":"Excluded sensitive path"}]));
}

#[test]
fn grouped_reads_and_completion_are_atomic_scoped_and_idempotent() {
    let mut f = fixture("old\n","new\n"); f.run.files.clear(); f.run.state = ReviewRunState::Preparing;
    let receipts: Vec<_> = ["a.rs","b.rs","c.rs"].iter().map(|path| {
        fs::write(f.root.join(path),"new\n").unwrap(); receipt(&f.run,&f.root,path,"old\n","new\n",path)
    }).collect();
    f.input = prepare_review(&mut f.run,&f.root,&f.storage,requirements(),&receipts,&[],&AtomicBool::new(false)).unwrap();
    assert_eq!(f.input.batches.len(),1);
    let mut corrupt = f.input.clone(); let id = f.run.files[2].id.clone();
    corrupt.diffs.get_mut(&id).unwrap()[0].content_hash = "fabricated".into();
    let untouched = f.run.clone();
    assert!(review_batch_page(&mut f.run,&corrupt,&f.storage,0,0).is_err());
    assert_eq!(f.run,untouched);
    let read = review_batch_page(&mut f.run,&f.input,&f.storage,0,0).unwrap();
    assert_eq!(read["diffs"].as_array().unwrap().len(),3);
    assert!(f.run.files.iter().all(|file| file.consumed_pages.contains(&0)));
    let ids:Vec<_> = f.run.files.iter().map(|f|f.id.clone()).collect();
    let mut bad = ids.clone(); bad.push("foreign".into()); let untouched = f.run.clone();
    assert!(apply_review_report(&mut f.run,&f.input,&f.storage,ReviewReport::FilesComplete{file_ids:bad},review_now()).is_err());
    assert_eq!(f.run,untouched);
    assert!(apply_review_report(&mut f.run,&f.input,&f.storage,ReviewReport::FilesComplete{file_ids:vec![ids[0].clone();2]},review_now()).is_err());
    for _ in 0..2 { apply_review_report(&mut f.run,&f.input,&f.storage,ReviewReport::FilesComplete{file_ids:ids.clone()},review_now()).unwrap(); }
    assert_eq!(f.run.completed_files(),3);
    assert!(review_batch_page(&mut f.run,&f.input,&f.storage,999,0).is_err());
}

#[test]
fn oversized_group_pages_require_all_pages_before_completion() {
    let mut f = fixture("old\n", &format!("{}\n", "large change".repeat(8000)));
    let result = review_batch_page(&mut f.run,&f.input,&f.storage,0,0).unwrap();
    let pages = result["total_pages"].as_u64().unwrap(); assert!(pages > 1);
    let id = f.run.files[0].id.clone();
    assert!(apply_review_report(&mut f.run,&f.input,&f.storage,ReviewReport::FilesComplete{file_ids:vec![id.clone()]},review_now()).is_err());
    for page in 1..pages { let result=review_batch_page(&mut f.run,&f.input,&f.storage,0,page as usize).unwrap();
        assert!(result["diffs"].as_array().unwrap().iter().map(|diff|diff["text"].as_str().unwrap().chars().count()).sum::<usize>() <= DIFF_PAGE_CHARS); }
    apply_review_report(&mut f.run,&f.input,&f.storage,ReviewReport::FilesComplete{file_ids:vec![id]},review_now()).unwrap();
    assert_eq!(f.run.completed_files(),1);
}

#[test]
fn scope_deadline_is_bounded_counts_unique_paths_and_includes_preparation() {
    let mut f = fixture("old\n", "new\n");
    let started = f.run.started_at;
    f.run.set_scope_deadline(&f.input);
    assert_eq!(f.run.deadline_at, started + 300);
    let file = f.run.files[0].clone();
    f.run.files = (0..63).map(|i| { let mut f=file.clone(); f.path=format!("file-{i}.rs").into(); f }).collect();
    f.run.set_scope_deadline(&f.input);
    assert_eq!(f.run.deadline_at, started + 756);
    f.run.files = vec![file.clone(); 90];
    f.run.set_scope_deadline(&f.input);
    assert_eq!(f.run.deadline_at, started + 300, "Historical segments are not separate files");
    f.input.diffs.values_mut().next().unwrap()[0].characters = 800_001;
    f.run.set_scope_deadline(&f.input);
    assert_eq!(f.run.deadline_at, started + 801);
    f.input.diffs.values_mut().next().unwrap()[0].characters = usize::MAX;
    f.run.set_scope_deadline(&f.input);
    assert_eq!(f.run.deadline_at, started + 900);
    f.run.set_scope_deadline(&f.input);
    assert_eq!(f.run.deadline_at, started + 900, "Progress never resets the timer");
}

fn ledger_entry(agent_id: Uuid, before: &str, after: &str) -> crate::local_store::StoredChatFileLedgerEntry {
    crate::local_store::StoredChatFileLedgerEntry { agent_id, path:"shared.rs".into(), observed:false,
        additions:1, deletions:1, counts_unavailable:false, segments_json:"[]".into(),
        baseline_hash:Some(content_hash(before.as_bytes())), result_hash:Some(content_hash(after.as_bytes())),
        baseline_content:Some(before.into()), result_content:Some(after.into()), updated_at:1 }
}

#[test]
fn confirmed_ledger_recovers_integration_contents_but_never_observations_or_bad_hashes() {
    let f = fixture("old\n", "new\n");
    let mut entry = ledger_entry(f.run.parent_id, "old\n", "new\n");
    let recovered = confirmed_ledger_evidence(&entry, f.run.project_id, &f.root, &[]);
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].before.as_deref(), Some("old\n"));
    assert_eq!(recovered[0].key.agent_id, f.run.parent_id);
    assert_eq!(recovered[0].key.root, f.root);
    let mut typed = receipt(&f.run, &f.root, "shared.rs", "old\n", "new\n", "typed");
    typed.before_hash = None; typed.after_hash = None;
    assert!(confirmed_ledger_evidence(&entry, f.run.project_id, &f.root, &[typed]).is_empty(), "duplicate contents need no extra review");
    entry.observed = true;
    assert!(confirmed_ledger_evidence(&entry, f.run.project_id, &f.root, &[]).is_empty());
    entry.observed = false; entry.result_hash = Some("fabricated".into());
    assert!(confirmed_ledger_evidence(&entry, f.run.project_id, &f.root, &[]).is_empty());
    entry.result_hash = None; entry.segments_json = "malformed".into();
    assert!(confirmed_ledger_evidence(&entry, f.run.project_id, &f.root, &[]).is_empty());
}

#[test]
fn persisted_integration_evidence_is_reviewable_and_ledger_updates_invalidate_scope() {
    use crate::{AgentAccessMode, AgentKind, AgentModel, AgentRecord, ProjectId};
    let mut f = fixture("old\n", "new\n");
    let store = LocalStore::open(f._dir.path().join("store")).unwrap();
    let model = AgentModel::default_for(AgentKind::Codex);
    let mut project = crate::Project::from_path(f.root.clone());
    project.id = ProjectId(f.run.project_id);
    let mut config = crate::AppConfig::default(); config.projects = vec![project];
    store.save_workspace_config(&config).unwrap();
    let mut parent = AgentRecord::new(ProjectId(f.run.project_id), f.root.clone(), "Parent", "",
        AgentKind::Codex, model, model.default_effort(), AgentAccessMode::FullAccess);
    parent.id = f.run.parent_id;
    store.save_agents(&[parent]).unwrap();
    let entry = ledger_entry(f.run.parent_id, "old\n", "new\n");
    store.replace_chat_file_ledger(f.run.parent_id, 1, &[entry.clone()]).unwrap();
    let (owned, limitations, revision) = store.review_ownership(f.run.parent_id).unwrap();
    assert!(limitations.is_empty(), "{limitations:?}"); assert_eq!(owned.len(), 1);
    assert_eq!(owned[0].key.project_id, f.run.project_id);
    assert_eq!(store.review_mutation_revision(f.run.parent_id).unwrap(), revision);
    f.run.files.clear(); f.run.state = ReviewRunState::Preparing;
    let input = prepare_review(&mut f.run, &f.root, &f.storage, requirements(), &owned, &limitations, &AtomicBool::new(false)).unwrap();
    f.run.mutation_revision = revision;
    assert_eq!(f.run.total_files(), 1); assert!(input.patch_only_files.is_empty());
    store.replace_chat_file_ledger(f.run.parent_id, 2, &[entry]).unwrap();
    let latest = store.review_mutation_revision(f.run.parent_id).unwrap();
    revalidate_review(&mut f.run, &input, latest);
    assert!(matches!(f.run.freshness, ReviewFreshness::Outdated(_)));
    assert!(!f.run.is_clean());
}

#[test]
fn source_content_budget_and_non_text_surrounding_files_are_explicit() {
    let f = fixture("old\n", "new\n");
    let text = "x".repeat(1024 * 1024);
    for i in 0..101 {
        fs::write(f.root.join(format!("source-{i:03}.rs")), &text).unwrap();
    }
    fs::write(f.root.join("binary.dat"), [0, 1, 2]).unwrap();
    fs::write(
        f.root.join("oversized.rs"),
        "x".repeat(MAX_SOURCE_BYTES + 1),
    )
    .unwrap();
    let mut run = ReviewRun::new(
        f.run.project_id,
        f.run.parent_id,
        "Claude".into(),
        "model".into(),
        "effort".into(),
        review_now(),
    );
    let input = prepare_review(
        &mut run,
        &f.root,
        &f.storage,
        requirements(),
        &[],
        &[],
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(
        input
            .source
            .keys()
            .filter(|p| p.to_string_lossy().starts_with("source-"))
            .count(),
        99
    );
    assert!(input.omitted_source.values().any(|r| r.contains("100 MiB")));
    assert_eq!(
        input.omitted_source[Path::new("binary.dat")],
        "Binary or non-UTF-8 source"
    );
    assert!(input.omitted_source[Path::new("oversized.rs")].contains("2 MiB"));
    assert!(run.limitations.iter().any(|l| l.contains("100 MiB")));
    let before = "before".repeat(MAX_SOURCE_BYTES / 6);
    let after = "after!".repeat(MAX_SOURCE_BYTES / 6);
    fs::write(f.root.join("z-owned.rs"), &after).unwrap();
    let mut run = ReviewRun::new(
        f.run.project_id,
        f.run.parent_id,
        "Claude".into(),
        "model".into(),
        "effort".into(),
        review_now(),
    );
    let owned = receipt(
        &run,
        &f.root,
        "z-owned.rs",
        &before,
        &after,
        "large-owned-edit",
    );
    let input = prepare_review(
        &mut run,
        &f.root,
        &f.storage,
        requirements(),
        &[owned],
        &[],
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(run.files.len(), 1);
    assert_eq!(run.files[0].status, ReviewFileStatus::Pending);
    assert!(input.source.contains_key(Path::new("z-owned.rs")));
    assert!(run.files[0].diff_pages > 1);
    // Paging keeps the manifest small even when a large patch is in scope.
    assert!(serde_json::to_vec(&input).unwrap().len() < 1024 * 1024);
    let page = &input.diffs[&run.files[0].id][0];
    fs::write(
        f.storage.join("blobs").join(&page.content_hash),
        "tampered page",
    )
    .unwrap();
    let id = run.files[0].id.clone();
    assert!(review_diff_page(&mut run, &input, &f.storage, &id, 0).is_err());
    assert!(run.files[0].consumed_pages.is_empty());
}

#[test]
fn startup_interrupts_unopened_conversations_and_preserves_finished_reviews() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalStore::open(dir.path().join("store")).unwrap();
    let runs: Vec<_> = (0..3)
        .map(|_| {
            ReviewRun::new(
                Uuid::new_v4(),
                Uuid::new_v4(),
                "Claude".into(),
                "model".into(),
                "effort".into(),
                review_now(),
            )
        })
        .collect();
    for run in &runs {
        store.create_review_run(run).unwrap();
    }
    store
        .transact_review(runs[2].id, None, |run| {
            run.stop(
                ReviewRunState::Cancelled,
                Some("Cancelled by user".into()),
                review_now(),
            );
            Ok(())
        })
        .unwrap();
    let finished = store.load_review_run(runs[2].id).unwrap();
    let interrupted = store.interrupt_all_unfinished_reviews().unwrap();
    assert_eq!(interrupted.len(), 2);
    assert!(interrupted
        .iter()
        .all(|r| r.state == ReviewRunState::Interrupted && !r.is_clean()));
    assert_eq!(store.load_review_run(finished.id).unwrap(), finished);
    assert!(store.interrupt_all_unfinished_reviews().unwrap().is_empty());
}

#[test]
fn all_provider_reviewer_records_preserve_selection_without_saved_mutation_authority() {
    use crate::{AgentAccessMode, AgentEffort, AgentKind, AgentModel, AgentRecord, ProjectId};
    for (provider, model) in [
        (AgentKind::Codex, AgentModel::CodexGpt61Sol),
        (AgentKind::Claude, AgentModel::ClaudeOpus55),
        (AgentKind::Gemini, AgentModel::default_for(AgentKind::Gemini)),
        (AgentKind::OpenCode, AgentModel::OpenCode),
    ] {
        let mut parent = AgentRecord::new(
            ProjectId(Uuid::new_v4()),
            Path::new("/workspace/repository").into(),
            "Coding",
            "Unrelated coding prompt",
            provider,
            model,
            AgentEffort::High,
            AgentAccessMode::FullAccess,
        );
        if provider == AgentKind::OpenCode {
            parent.set_external_model("openrouter/vendor/model", "Selected model", vec!["high".into(), "medium".into()]);
            parent.effort = AgentEffort::High;
        }
        parent.chat_session_id = Some("old-chat-session".into());
        parent.cli_session_id = Some("old-cli-session".into());
        parent.notes = "Unrelated services".into();
        parent.linked_docs.push("credentials.json".into());
        let run = ReviewRun::new(
            parent.project_id.0,
            parent.id,
            "provider".into(),
            "model".into(),
            "effort".into(),
            review_now(),
        );
        let reviewer =
            fresh_reviewer_record(&parent, &run, "/app-data/review/runtime".into()).unwrap();
        assert_ne!(reviewer.id, parent.id);
        assert_eq!(reviewer.id, run.reviewer_id);
        assert_eq!(reviewer.provider, parent.provider);
        assert_eq!(reviewer.runtime, crate::AgentRuntimeKind::Chat);
        assert_eq!(reviewer.model, parent.model);
        assert_eq!(reviewer.effort, parent.effort);
        assert_eq!(reviewer.model_cli_value(), parent.model_cli_value());
        assert_eq!(reviewer.external_model_variants, parent.external_model_variants);
        assert_eq!(reviewer.model_label(), parent.model_label());
        assert!(
            reviewer.chat_session_id.is_none()
                && reviewer.cli_session_id.is_none()
                && reviewer.delegation.is_none()
                && reviewer.expert_snapshot.is_none()
                && reviewer.linked_docs.is_empty()
                && reviewer.notes.is_empty()
        );
        assert_eq!(reviewer.doc, REVIEW_INSTRUCTIONS);
        assert_eq!(reviewer.access_mode, AgentAccessMode::AskForApproval);
        let mut value = serde_json::to_value(reviewer).unwrap();
        assert!(value.get("review_run_id").is_none());
        value["review_run_id"] = serde_json::json!(run.id);
        assert!(serde_json::from_value::<AgentRecord>(value)
            .unwrap()
            .review_run_id
            .is_none());
        assert!(
            fresh_reviewer_record(&parent, &run, "/workspace/repository/review".into()).is_err()
        );
    }
}
