use std::cmp::Ordering;

use ide_core::git::BranchInfo;

/// Keep the common integration branches first in every branch picker.
/// Only remote refs lose their remote prefix: `feature/main` is not `main`.
pub(crate) fn branch_priority(name: &str, is_remote: bool) -> u8 {
    let name = if is_remote {
        name.split_once('/').map_or(name, |(_, branch)| branch)
    } else {
        name
    };
    match name {
        "dev" => 0,
        "main" => 1,
        "staging" => 2,
        _ => 3,
    }
}

/// Preserve current/local/recent ordering after the pinned branches.
pub(crate) fn compare_branches(a: &BranchInfo, b: &BranchInfo) -> Ordering {
    branch_priority(&a.name, a.is_remote)
        .cmp(&branch_priority(&b.name, b.is_remote))
        .then_with(|| (!a.is_head, a.is_remote).cmp(&(!b.is_head, b.is_remote)))
        .then_with(|| b.tip_time.cmp(&a.tip_time))
        .then_with(|| a.name.cmp(&b.name))
}
