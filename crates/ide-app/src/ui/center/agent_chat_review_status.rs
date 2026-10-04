//! What the review UI may claim, derived only from the controller's run.
//! Nothing here estimates progress or infers a clean result: counts come from
//! `ReviewRun`, and anything short of `is_clean()` says so in words.
use ide_core::code_review::{
    ReviewFileStatus, ReviewFreshness, ReviewRun, ReviewRunState, ReviewStage,
};

/// The compact panel that holds the composer while a reviewer process lives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ReviewProgress {
    pub title: &'static str,
    pub scope: String,
    pub detail: String,
    pub group: Option<String>,
    pub open_changes: Option<String>,
    pub findings: usize,
    pub cancelling: bool,
}

pub(super) fn stage_label(stage: ReviewStage) -> &'static str {
    match stage {
        ReviewStage::Understanding => "Understanding",
        ReviewStage::Reviewing => "Reviewing",
        ReviewStage::CheckingFindings => "Checking findings",
    }
}

fn stage_progress(stage: ReviewStage) -> String {
    let step = match stage {
        ReviewStage::Understanding => 1,
        ReviewStage::Reviewing => 2,
        ReviewStage::CheckingFindings => 3,
    };
    format!("Step {step}/3 · {}", stage_label(stage))
}

/// Batches are grouped by parent directory; an empty group is the
/// repository root, and no group means no batch is in progress yet.
pub(super) fn group_label(group: Option<&str>) -> Option<String> {
    let group = group?.trim().trim_end_matches('/');
    Some(if group.is_empty() || group == "." {
        "the project root".to_string()
    } else {
        group.to_string()
    })
}

fn files(count: usize) -> String {
    if count == 1 {
        "1 file".into()
    } else {
        format!("{count} files")
    }
}

pub(super) fn unavailable_reason(reason: &str) -> &str {
    match reason {
        "Confirmed mutation lacks inspectable before/after contents" => "Earlier edit contents were not saved",
        _ => reason,
    }
}

pub(super) fn review_progress(run: &ReviewRun) -> ReviewProgress {
    let total = run.total_files();
    let done = run.completed_files();
    let cancelling = run.state == ReviewRunState::Cancelling;
    let (title, detail) = if cancelling {
        ("Cancelling review", "Waiting for the reviewer to stop".to_string())
    } else if run.state.terminal() {
        ("Finishing review", "Stopping the reviewer".to_string())
    } else if run.state == ReviewRunState::Preparing && total == 0 {
        ("Reviewing code", format!("{} · Preparing snapshot", stage_progress(run.stage)))
    } else if total == 0 {
        ("Reviewing code", stage_progress(run.stage))
    } else {
        (
            "Reviewing code",
            format!("{} · {done} of {}", stage_progress(run.stage), files(total)),
        )
    };
    ReviewProgress {
        title,
        scope: if total == 0 {
            "This conversation's changes".into()
        } else {
            format!("This conversation's changes, {}", files(total))
        },
        detail,
        group: if run.state.terminal() || cancelling {
            None
        } else {
            group_label(run.current_group.as_deref())
        },
        open_changes: if run.state.terminal() || cancelling { None } else {
            let paths = run.files.iter().filter(|file| file.status == ReviewFileStatus::Reviewing)
                .map(|file| file.path.display().to_string()).collect::<std::collections::BTreeSet<_>>();
            paths.iter().next().map(|path| if paths.len() == 1 { path.clone() }
                else { format!("{path} (+{} more)", paths.len() - 1) })
        },
        findings: run.findings.len(),
        cancelling,
    }
}

/// How a review card's state reads. Only `Clean` may look reassuring.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ReviewTone {
    Live,
    Clean,
    Findings,
    Incomplete,
    Failed,
    Stale,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ReviewOutcome {
    pub state: &'static str,
    pub tone: ReviewTone,
    pub summary: String,
    pub notice: Option<String>,
    pub coverage: String,
    pub limitations: Vec<String>,
    pub skipped: Vec<(String, String)>,
    pub unreviewed: Vec<String>,
    pub reviewed: Vec<String>,
    /// Offer "Review latest changes" instead of fixing stale evidence.
    pub stale: bool,
    pub can_fix: bool,
}

pub(super) fn review_outcome(run: &ReviewRun, reviewer_active: bool) -> ReviewOutcome {
    let total = run.total_files();
    let done = run.completed_files();
    let findings = run.findings.len();
    let clean = run.is_clean();
    let finished_available = run.finalized_by_reviewer
        && matches!(run.state, ReviewRunState::Complete | ReviewRunState::Partial)
        && !run.files.is_empty()
        && run.files.iter().all(|file| matches!(file.status, ReviewFileStatus::Complete | ReviewFileStatus::Skipped));
    let complete_coverage = run.state == ReviewRunState::Complete
        && total > 0
        && done == total
        && run.limitations.is_empty();
    let (state, mut tone, mut notice): (&'static str, ReviewTone, Option<String>) = match run.state {
        ReviewRunState::Preparing | ReviewRunState::Running | ReviewRunState::Cancelling => (
            "Reviewing",
            ReviewTone::Live,
            Some("Checked findings appear here as the reviewer confirms them.".into()),
        ),
        ReviewRunState::Complete if clean => ("Complete", ReviewTone::Clean, None),
        ReviewRunState::Complete if complete_coverage => ("Complete", ReviewTone::Findings, None),
        ReviewRunState::Complete => (
            "Incomplete",
            ReviewTone::Incomplete,
            Some("Coverage is incomplete, so unreviewed code may still have problems.".into()),
        ),
        ReviewRunState::Partial if finished_available => (
            "Finished",
            ReviewTone::Incomplete,
            Some(if run.files.iter().any(|file| file.status == ReviewFileStatus::Complete) {
                "Reviewed all available changes. Some files or earlier edits were unavailable; the coverage gaps are listed below.".into()
            } else {
                "The review finished, but no recorded changes were readable. The unavailable changes are listed below.".into()
            }),
        ),
        ReviewRunState::Partial => (
            "Partial",
            ReviewTone::Incomplete,
            Some(if run.limitations.iter().any(|reason| reason.contains("deadline reached") || reason.contains("deadline")) {
                let minutes = run.deadline_at.saturating_sub(run.started_at).saturating_add(59) / 60;
                format!("The review reached its {}-minute limit. {} of {} files were checked; {} still need review.", minutes, done, total, total.saturating_sub(done))
            } else if done == total && total > 0 {
                "The available files were checked, but gaps in the recorded changes prevent a complete result.".into()
            } else {
                "The review stopped before covering every file. Unchecked changes still need review.".into()
            }),
        ),
        ReviewRunState::Cancelled => (
            "Cancelled",
            ReviewTone::Incomplete,
            Some("You cancelled this review. Results cover only what was checked.".into()),
        ),
        ReviewRunState::Failed => (
            "Failed",
            ReviewTone::Failed,
            Some("The review failed. Results cover only what was checked before it stopped.".into()),
        ),
        ReviewRunState::Interrupted => (
            "Interrupted",
            ReviewTone::Failed,
            Some(
                "Choro closed before the review finished. Results cover only what was checked."
                    .into(),
            ),
        ),
    };
    let stale = run.state.terminal() && run.freshness != ReviewFreshness::Current;
    if stale {
        let reason = match &run.freshness {
            ReviewFreshness::Outdated(reason) => {
                state_notice("The code changed after this review", reason)
            }
            ReviewFreshness::Uncertain(reason) => {
                state_notice("Choro can't confirm this review matches the current code", reason)
            }
            ReviewFreshness::Current => unreachable!(),
        };
        notice = Some(match notice {
            Some(previous) => format!("{reason} {previous}"),
            None => reason,
        });
        tone = ReviewTone::Stale;
    }
    let summary = if clean {
        "No findings".to_string()
    } else if findings == 1 {
        "1 finding".to_string()
    } else if findings > 0 {
        format!("{findings} findings")
    } else if finished_available && run.files.iter().any(|file| file.status == ReviewFileStatus::Complete) {
        "No findings in reviewed changes".to_string()
    } else if finished_available {
        "No reviewable changes".to_string()
    } else if run.state.terminal() {
        "No findings confirmed".to_string()
    } else {
        "No findings yet".to_string()
    };
    let coverage = if total == 0 {
        if run.state.terminal() {
            "No files were reviewed".to_string()
        } else {
            "Preparing the file list".to_string()
        }
    } else if done == total && total == 1 {
        "Reviewed 1 file".to_string()
    } else if done == total {
        format!("Reviewed all {}", files(total))
    } else {
        format!("Reviewed {done} of {}", files(total))
    };
    let mut skipped = Vec::new();
    let mut unreviewed = Vec::new();
    let mut reviewed = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for file in &run.files {
        if !seen.insert(&file.path) {
            continue;
        }
        let parts = run.files.iter().filter(|f| f.path == file.path);
        let path = file.path.display().to_string();
        if let Some(skip) = parts.clone().find(|f| f.status == ReviewFileStatus::Skipped) {
            skipped.push((
                path,
                skip.skip_reason
                    .clone()
                    .filter(|r| !r.trim().is_empty())
                    .unwrap_or_else(|| "No reason recorded".into()),
            ));
        } else if run.state.terminal()
            && parts.clone().any(|f| f.status != ReviewFileStatus::Complete)
        {
            unreviewed.push(path);
        } else if parts.clone().all(|f| f.status == ReviewFileStatus::Complete) {
            reviewed.push(path);
        }
    }
    ReviewOutcome {
        state,
        tone,
        summary,
        notice,
        coverage,
        limitations: run.limitations.clone(),
        skipped,
        unreviewed,
        reviewed,
        stale,
        can_fix: !reviewer_active && run.can_fix() && findings > 0,
    }
}

fn state_notice(lead: &str, reason: &str) -> String {
    let reason = reason.trim().trim_end_matches('.');
    if reason.is_empty() {
        format!("{lead}.")
    } else {
        format!("{lead}: {reason}.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::code_review::{ReviewFile, ReviewRange, ReviewSide};
    use uuid::Uuid;

    fn file(path: &str, status: ReviewFileStatus) -> ReviewFile {
        ReviewFile {
            id: format!("{path}:{status:?}"),
            path: path.into(),
            change_kind: "modified".into(),
            attributed_ranges: vec![ReviewRange {
                side: ReviewSide::After,
                start: 1,
                end: 2,
            }],
            before_hash: None,
            after_hash: None,
            diff_pages: 1,
            consumed_pages: Default::default(),
            status,
            skip_reason: None,
        }
    }

    fn run_with(state: ReviewRunState, files: Vec<ReviewFile>) -> ReviewRun {
        let mut run = ReviewRun::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "Claude".into(),
            "model".into(),
            "effort".into(),
            1,
        );
        run.state = state;
        run.files = files;
        run.freshness = ReviewFreshness::Current;
        run
    }

    #[test]
    fn panel_reports_real_unique_file_coverage_stage_and_group() {
        let mut run = run_with(
            ReviewRunState::Running,
            vec![
                file("a.rs", ReviewFileStatus::Complete),
                file("b.rs", ReviewFileStatus::Complete),
                file("b.rs", ReviewFileStatus::Reviewing),
                file("c.rs", ReviewFileStatus::Pending),
            ],
        );
        run.stage = ReviewStage::Reviewing;
        run.current_group = Some("crates/ide-app/src/ui".into());
        let progress = review_progress(&run);
        // b.rs has an unfinished receipt segment, so only a.rs counts.
        assert_eq!(progress.detail, "Step 2/3 · Reviewing · 1 of 3 files");
        assert_eq!(progress.scope, "This conversation's changes, 3 files");
        assert_eq!(progress.group.as_deref(), Some("crates/ide-app/src/ui"));
        assert_eq!(progress.open_changes.as_deref(), Some("b.rs"));
        run.stage = ReviewStage::CheckingFindings;
        run.current_group = Some(String::new());
        let progress = review_progress(&run);
        assert_eq!(progress.detail, "Step 3/3 · Checking findings · 1 of 3 files");
        assert_eq!(
            progress.group.as_deref(),
            Some("the project root"),
            "root-directory batches get a readable label"
        );
        run.current_group = None;
        assert_eq!(review_progress(&run).group, None, "no batch, no label");
    }

    #[test]
    fn panel_never_invents_counts_while_preparing_or_stopping() {
        let run = run_with(ReviewRunState::Preparing, vec![]);
        let progress = review_progress(&run);
        assert_eq!(progress.detail, "Step 1/3 · Understanding · Preparing snapshot");
        assert!(!progress.detail.contains(" of "), "file coverage is unknown before capture");
        let mut cancelling = run.clone();
        cancelling.state = ReviewRunState::Cancelling;
        cancelling.current_group = Some("src".into());
        let progress = review_progress(&cancelling);
        assert!(progress.cancelling);
        assert_eq!(progress.title, "Cancelling review");
        assert_eq!(progress.group, None, "a stopping reviewer is not in a group");
        assert_eq!(progress.open_changes, None);
        let mut finished = run;
        finished.state = ReviewRunState::Complete;
        assert_eq!(review_progress(&finished).title, "Finishing review");
    }

    #[test]
    fn only_a_complete_current_review_with_full_coverage_reads_clean() {
        let clean = run_with(
            ReviewRunState::Complete,
            vec![file("a.rs", ReviewFileStatus::Complete)],
        );
        let outcome = review_outcome(&clean, false);
        assert_eq!(outcome.tone, ReviewTone::Clean);
        assert_eq!(outcome.summary, "No findings");
        assert_eq!(outcome.coverage, "Reviewed 1 file");
        assert_eq!(outcome.reviewed, ["a.rs"]);

        for state in [
            ReviewRunState::Partial,
            ReviewRunState::Cancelled,
            ReviewRunState::Failed,
            ReviewRunState::Interrupted,
        ] {
            let run = run_with(
                state,
                vec![
                    file("a.rs", ReviewFileStatus::Complete),
                    file("b.rs", ReviewFileStatus::Pending),
                ],
            );
            let outcome = review_outcome(&run, false);
            assert_ne!(outcome.tone, ReviewTone::Clean, "{state:?}");
            assert_ne!(outcome.summary, "No findings", "{state:?}");
            assert!(outcome.notice.is_some(), "{state:?}");
            assert_eq!(outcome.unreviewed, ["b.rs"], "{state:?}");
            assert_eq!(outcome.coverage, "Reviewed 1 of 2 files");
        }

        let mut limited = clean.clone();
        limited.limitations.push("Five-minute deadline reached".into());
        let outcome = review_outcome(&limited, false);
        assert_eq!(outcome.state, "Incomplete");
        assert_eq!(outcome.summary, "No findings confirmed");

        let mut skipped = clean.clone();
        skipped.files.push(ReviewFile {
            skip_reason: Some("Binary file".into()),
            ..file("logo.png", ReviewFileStatus::Skipped)
        });
        let outcome = review_outcome(&skipped, false);
        assert_ne!(outcome.tone, ReviewTone::Clean);
        assert_eq!(outcome.skipped, [("logo.png".into(), "Binary file".into())]);
        assert_eq!(outcome.coverage, "Reviewed 1 of 2 files");
    }

    #[test]
    fn deadline_notice_explains_the_stop_and_unchecked_coverage() {
        let mut run = run_with(ReviewRunState::Partial, vec![
            file("checked.rs", ReviewFileStatus::Complete), file("unchecked.rs", ReviewFileStatus::Pending)]);
        run.limitations.push("Five-minute deadline reached; coverage is incomplete".into());
        let outcome = review_outcome(&run, false);
        assert_eq!(outcome.notice.as_deref(), Some("The review reached its 5-minute limit. 1 of 2 files were checked; 1 still need review."));
        assert_eq!(outcome.reviewed, ["checked.rs"]);
        assert_eq!(outcome.unreviewed, ["unchecked.rs"]);
        assert_ne!(outcome.summary, "No findings");
        run.deadline_at = run.started_at + 756;
        assert_eq!(review_outcome(&run, false).notice.as_deref(), Some("The review reached its 13-minute limit. 1 of 2 files were checked; 1 still need review."));
    }

    #[test]
    fn finalized_available_changes_are_finished_even_with_missing_historical_edits() {
        let mut run = run_with(ReviewRunState::Partial,vec![
            file("a.rs",ReviewFileStatus::Complete),file("controller.rs",ReviewFileStatus::Complete),
            file("controller.rs",ReviewFileStatus::Skipped)]);
        run.finalized_by_reviewer = true;
        run.limitations.push("Missing earlier edit contents".into());
        let outcome = review_outcome(&run,false);
        assert_eq!(outcome.state,"Finished");
        assert_eq!(outcome.summary,"No findings in reviewed changes");
        assert_eq!(outcome.coverage,"Reviewed 1 of 2 files");
        assert!(outcome.notice.unwrap().starts_with("Reviewed all available changes."));
        assert_ne!(outcome.tone,ReviewTone::Clean);
        run.files[1].status = ReviewFileStatus::Pending;
        assert_eq!(review_outcome(&run,false).state,"Partial", "Unread assigned pages must not look finished");
        run.files.iter_mut().for_each(|file| file.status = ReviewFileStatus::Skipped);
        assert_eq!(review_outcome(&run,false).summary,"No reviewable changes");
        run.state = ReviewRunState::Cancelled;
        assert_eq!(review_outcome(&run,false).state,"Cancelled");
    }

    #[test]
    fn outdated_or_uncertain_results_offer_a_new_review_instead_of_fixes() {
        let mut run = run_with(
            ReviewRunState::Complete,
            vec![file("a.rs", ReviewFileStatus::Complete)],
        );
        run.findings.push(ide_core::code_review::ReviewFinding {
            id: "f1".into(),
            severity: ide_core::code_review::ReviewSeverity::High,
            location: ide_core::code_review::ReviewLocation {
                file_id: "a".into(),
                path: "a.rs".into(),
                side: ReviewSide::After,
                start: 1,
                end: 1,
            },
            title: "Lost update".into(),
            trigger: "t".into(),
            consequence: "c".into(),
            suggested_fix: "f".into(),
            evidence: vec![],
            challenge: "ch".into(),
        });
        assert!(review_outcome(&run, false).can_fix);
        assert!(
            !review_outcome(&run, true).can_fix,
            "a live reviewer process still holds the conversation"
        );
        for freshness in [
            ReviewFreshness::Outdated("a.rs changed".into()),
            ReviewFreshness::Uncertain("Receipts unavailable".into()),
        ] {
            run.freshness = freshness;
            let outcome = review_outcome(&run, false);
            assert!(outcome.stale);
            assert!(!outcome.can_fix);
            assert_eq!(outcome.tone, ReviewTone::Stale);
            assert!(outcome.notice.unwrap().ends_with('.'));
        }
    }
}
