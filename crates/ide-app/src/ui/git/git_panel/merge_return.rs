//! The optional local follow-up to a pull request merge: return the checkout
//! to the branch the PR merged into and pull the merged result, so the next
//! piece of work starts from up-to-date code.

use std::path::Path;

use ide_core::git::merge_return::{return_to_base_after_merge, ReturnOutcome};

use super::pull_request_support::MergePullRequestOutcome;

/// Checkbox label and supporting line shown in every Merge PR dialog.
pub(crate) fn return_to_base_option(base_branch: &str) -> (String, String) {
    (
        format!("Return to {base_branch} and pull latest"),
        format!(
            "After merging, switch this checkout to {base_branch} and pull the merged changes. \
             Skipped if you've moved to another branch; local changes are never discarded."
        ),
    )
}

/// Runs the local follow-up when requested. The merge has already succeeded
/// at this point, so a local failure is recorded on the outcome (and shown as
/// a warning) rather than reported as a failed merge.
pub(crate) fn with_local_return(
    outcome: MergePullRequestOutcome,
    repo: &Path,
    requested: bool,
) -> MergePullRequestOutcome {
    if !requested {
        return outcome;
    }
    let local_return = return_to_base_after_merge(repo, &outcome.branch, &outcome.base_branch)
        .map_err(|error| format!("{error:#}"));
    outcome.with_local_return_result(local_return)
}

/// Appends the local follow-up's result to the merge summary.
pub(super) fn with_local_return_sentence(
    summary: String,
    local: &Result<ReturnOutcome, String>,
) -> String {
    let sentence = match local {
        Ok(ReturnOutcome::Updated {
            base,
            switched: true,
        }) => format!("Switched to {base} and pulled the latest."),
        Ok(ReturnOutcome::Updated {
            base,
            switched: false,
        }) => format!("Pulled the latest {base}."),
        Ok(ReturnOutcome::Stayed { current }) => {
            format!("Stayed on {current} because it isn't the merged branch.")
        }
        Err(error) => format!("Your checkout was not updated: {}", error.trim()),
    };
    let summary = summary.trim_end();
    if summary.ends_with('.') {
        format!("{summary} {sentence}")
    } else {
        format!("{summary}. {sentence}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn option_names_the_destination_branch() {
        let (label, description) = return_to_base_option("dev");
        assert_eq!(label, "Return to dev and pull latest");
        assert!(description.contains("switch this checkout to dev"));
        assert!(description.contains("never discarded"));
    }

    #[test]
    fn summary_describes_each_local_result() {
        let merged = "Merged pull request #7 into main".to_string();
        assert_eq!(
            with_local_return_sentence(
                merged.clone(),
                &Ok(ReturnOutcome::Updated {
                    base: "main".into(),
                    switched: true
                })
            ),
            "Merged pull request #7 into main. Switched to main and pulled the latest."
        );
        assert_eq!(
            with_local_return_sentence(
                merged.clone(),
                &Ok(ReturnOutcome::Updated {
                    base: "main".into(),
                    switched: false
                })
            ),
            "Merged pull request #7 into main. Pulled the latest main."
        );
        assert_eq!(
            with_local_return_sentence(
                merged.clone(),
                &Ok(ReturnOutcome::Stayed {
                    current: "feat/next".into()
                })
            ),
            "Merged pull request #7 into main. Stayed on feat/next because it isn't the merged branch."
        );
        assert_eq!(
            with_local_return_sentence(
                "Merged #7 and deleted feat/x on GitHub. Your local branch is kept.".into(),
                &Err("Couldn't switch to main: local changes\n".into())
            ),
            "Merged #7 and deleted feat/x on GitHub. Your local branch is kept. \
             Your checkout was not updated: Couldn't switch to main: local changes"
        );
    }
}
