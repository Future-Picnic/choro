use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use uuid::Uuid;

const MAX_LEDGER_CONTENT_BYTES: usize = 2 * 1024 * 1024;
const MAX_LEDGER_DIFF_LINES: usize = 2_000;

/// Precomputed paths used to hide internal visualization artifacts from
/// changed-file summaries. Constructing this must stay cheap because chat rows
/// consult it while scrolling.
#[derive(Clone, Debug)]
pub(crate) struct VisualizationArtifactFilter {
    repo_path: PathBuf,
    visualization_dir: PathBuf,
}

impl VisualizationArtifactFilter {
    pub(crate) fn new(agent_id: Uuid, repo_path: &Path) -> Self {
        let app_data_root = ide_core::AppConfig::config_path()
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        Self {
            repo_path: normalize_path(repo_path),
            visualization_dir: app_data_root
                .join("data")
                .join("agents")
                .join(agent_id.to_string())
                .join("artifacts")
                .join("visualizations"),
        }
    }

    pub(crate) fn is_artifact(&self, path: &Path) -> bool {
        is_visualization_artifact_with_root(&self.repo_path, Some(&self.visualization_dir), path)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChangedFilesSummary {
    /// Paths reported by a structured mutation event owned by this agent
    /// (for example Codex `fileChange` or Claude `Edit`). These are the only
    /// files described as "edited" in the UI.
    pub files: Vec<FileChangeStat>,
    /// Paths observed around a command/generator execution. The causal window
    /// is useful feedback, but it is deliberately kept separate because a
    /// shared worktree may have changed concurrently.
    pub observed_files: Vec<FileChangeStat>,
    /// Stable provider turn identity. Old persisted summaries have no id and
    /// are restored as legacy observations instead of being promoted to exact
    /// edits.
    pub turn_id: Option<String>,
    /// Version 1 means the provider supplied agent-action attribution. Zero is
    /// reserved for pre-ledger persisted events.
    pub attribution_version: u8,
    /// Monotonic chat-local projection revision. It is not a provider turn id;
    /// it exists so asynchronous persistence can reject a stale completion.
    pub ledger_revision: u64,
    pub snapshot_id: Option<Uuid>,
    pub commit_sha: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileChangeStat {
    pub path: PathBuf,
    pub additions: usize,
    pub deletions: usize,
    /// The counts describe the current worktree projection rather than a
    /// turn-local delta. Repeated projections replace earlier totals so they
    /// are never added twice.
    pub counts_are_projection: bool,
    /// Optional hashes let capable adapters identify an exact edit that
    /// returns a file to the chat's original content without persisting file
    /// contents in the timeline.
    pub baseline_hash: Option<String>,
    pub result_hash: Option<String>,
    /// Bounded virtual projection used only by the cumulative chat ledger.
    /// Timeline serialization intentionally omits source contents; the v30
    /// ledger table persists them for exact net stats across app restarts.
    pub baseline_content: Option<String>,
    pub result_content: Option<String>,
}

/// One file mutation surfaced while a provider turn is still running. This is
/// deliberately separate from [`ChangedFilesSummary`]: activities are the
/// lightweight chronological audit trail, while the summary is the immutable
/// receipt folded into the chat-wide ledger when the turn closes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileChangeActivity {
    /// Stable tool-action + path identity so streaming updates replace one row
    /// instead of producing duplicates.
    pub id: String,
    pub turn_id: String,
    pub file: FileChangeStat,
    pub observed: bool,
    pub updated_at: u64,
}

impl FileChangeActivity {
    pub fn new(
        id: impl Into<String>,
        turn_id: impl Into<String>,
        file: FileChangeStat,
        observed: bool,
        updated_at: u64,
    ) -> Self {
        Self {
            id: id.into(),
            turn_id: turn_id.into(),
            file,
            observed,
            updated_at,
        }
    }
}

impl ChangedFilesSummary {
    pub fn attributed(
        turn_id: impl Into<String>,
        files: Vec<FileChangeStat>,
        observed_files: Vec<FileChangeStat>,
    ) -> Self {
        Self {
            files,
            observed_files,
            turn_id: Some(turn_id.into()),
            attribution_version: 1,
            ..Default::default()
        }
    }

    pub fn total_additions(&self) -> usize {
        self.files.iter().map(|file| file.additions).sum()
    }

    pub fn total_deletions(&self) -> usize {
        self.files.iter().map(|file| file.deletions).sum()
    }

    pub fn total_observed_additions(&self) -> usize {
        self.observed_files.iter().map(|file| file.additions).sum()
    }

    pub fn total_observed_deletions(&self) -> usize {
        self.observed_files.iter().map(|file| file.deletions).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty() && self.observed_files.is_empty()
    }

    pub fn from_activities<'a>(
        turn_id: impl Into<String>,
        activities: impl IntoIterator<Item = &'a FileChangeActivity>,
    ) -> Self {
        let turn_id = turn_id.into();
        let mut exact = BTreeMap::new();
        let mut observed = BTreeMap::new();
        for activity in activities {
            if activity.turn_id != turn_id {
                continue;
            }
            if activity.observed {
                if !exact.contains_key(&activity.file.path) {
                    merge_file_stat(&mut observed, &activity.file);
                }
            } else {
                observed.remove(&activity.file.path);
                merge_file_stat(&mut exact, &activity.file);
            }
        }
        Self::attributed(
            turn_id,
            exact.into_values().collect(),
            observed.into_values().collect(),
        )
    }

    pub fn remove_visualization_artifacts(&mut self, agent_id: Uuid, repo_path: &Path) {
        let filter = VisualizationArtifactFilter::new(agent_id, repo_path);
        self.files.retain(|file| !filter.is_artifact(&file.path));
        self.observed_files
            .retain(|file| !filter.is_artifact(&file.path));
    }

    /// Fold one immutable turn receipt into the chat-wide net ledger. Exact
    /// edits always win over observations for the same path. When an adapter
    /// provides content hashes, returning to the first baseline removes the
    /// cumulative entry while the historical receipt remains untouched.
    pub fn merge_turn(&mut self, turn: &Self) {
        self.ledger_revision = if turn.ledger_revision == 0 {
            self.ledger_revision.saturating_add(1)
        } else {
            self.ledger_revision.max(turn.ledger_revision)
        };
        let mut exact = self
            .files
            .drain(..)
            .map(|file| (file.path.clone(), file))
            .collect::<BTreeMap<_, _>>();
        let mut observed = self
            .observed_files
            .drain(..)
            .map(|file| (file.path.clone(), file))
            .collect::<BTreeMap<_, _>>();

        for file in &turn.files {
            observed.remove(&file.path);
            merge_file_stat(&mut exact, file);
        }
        for file in &turn.observed_files {
            if !exact.contains_key(&file.path) {
                merge_file_stat(&mut observed, file);
            }
        }

        self.files = exact.into_values().collect();
        self.observed_files = observed.into_values().collect();
        self.attribution_version = self.attribution_version.max(turn.attribution_version);
        self.snapshot_id = turn.snapshot_id.or(self.snapshot_id);
        if turn.commit_sha.is_some() {
            self.commit_sha.clone_from(&turn.commit_sha);
        }
    }
}

fn merge_file_stat(files: &mut BTreeMap<PathBuf, FileChangeStat>, next: &FileChangeStat) {
    let baseline = files
        .get(&next.path)
        .and_then(|existing| existing.baseline_hash.clone())
        .or_else(|| next.baseline_hash.clone());
    let baseline_content = files
        .get(&next.path)
        .and_then(|existing| existing.baseline_content.clone())
        .or_else(|| bounded_content(next.baseline_content.clone()));
    let result_content = bounded_content(next.result_content.clone()).or_else(|| {
        files
            .get(&next.path)
            .and_then(|existing| existing.result_content.clone())
    });
    let returned_to_baseline = match (baseline.as_ref(), next.result_hash.as_ref()) {
        (Some(baseline), Some(result)) => baseline == result,
        _ => baseline_content.is_some() && result_content == baseline_content,
    };
    if returned_to_baseline {
        files.remove(&next.path);
        return;
    }

    let net_counts = baseline_content
        .as_deref()
        .zip(result_content.as_deref())
        .and_then(|(baseline, result)| bounded_line_diff_counts(baseline, result));

    match files.get_mut(&next.path) {
        Some(existing) => {
            if let Some((additions, deletions)) = net_counts {
                existing.additions = additions;
                existing.deletions = deletions;
            } else if next.counts_are_projection {
                existing.additions = next.additions;
                existing.deletions = next.deletions;
            } else {
                existing.additions = existing.additions.saturating_add(next.additions);
                existing.deletions = existing.deletions.saturating_add(next.deletions);
            }
            existing.baseline_hash = baseline;
            existing.baseline_content = baseline_content;
            existing.result_content = result_content;
            if next.result_hash.is_some() {
                existing.result_hash.clone_from(&next.result_hash);
            }
        }
        None => {
            let mut file = next.clone();
            file.baseline_content = baseline_content;
            file.result_content = result_content;
            if let Some((additions, deletions)) = net_counts {
                file.additions = additions;
                file.deletions = deletions;
            }
            files.insert(next.path.clone(), file);
        }
    }
}

fn bounded_content(content: Option<String>) -> Option<String> {
    content.filter(|content| content.len() <= MAX_LEDGER_CONTENT_BYTES)
}

/// Count the shortest line edit script without retaining diff hunks. The
/// projection is capped so a generated file cannot make a foreground ledger
/// update quadratic in the file size.
fn bounded_line_diff_counts(baseline: &str, result: &str) -> Option<(usize, usize)> {
    let baseline = baseline.lines().collect::<Vec<_>>();
    let result = result.lines().collect::<Vec<_>>();
    if baseline.len().max(result.len()) > MAX_LEDGER_DIFF_LINES {
        return None;
    }
    let n = baseline.len() as isize;
    let m = result.len() as isize;
    let max = (n + m) as usize;
    if max == 0 {
        return Some((0, 0));
    }
    let offset = max as isize;
    let mut frontier = vec![0isize; max * 2 + 3];
    for distance in 0..=max {
        let distance = distance as isize;
        let mut diagonal = -distance;
        while diagonal <= distance {
            let index = (diagonal + offset + 1) as usize;
            let mut x = if diagonal == -distance
                || (diagonal != distance && frontier[index - 1] < frontier[index + 1])
            {
                frontier[index + 1]
            } else {
                frontier[index - 1] + 1
            };
            let mut y = x - diagonal;
            while x < n && y < m && baseline[x as usize] == result[y as usize] {
                x += 1;
                y += 1;
            }
            frontier[index] = x;
            if x >= n && y >= m {
                let deletions = ((distance + diagonal) / 2) as usize;
                let additions = ((distance - diagonal) / 2) as usize;
                return Some((additions, deletions));
            }
            diagonal += 2;
        }
    }
    None
}

fn is_visualization_artifact_with_root(
    repo_path: &Path,
    visualization_dir: Option<&Path>,
    path: &Path,
) -> bool {
    let repo_relative = normalize_path(path.strip_prefix(repo_path).unwrap_or(path));
    if ide_core::git::is_internal_visualization_path(&repo_relative) {
        return true;
    }

    let absolute = if path.is_absolute() {
        normalize_path(path)
    } else {
        normalize_path(&repo_path.join(path))
    };
    visualization_dir.is_some_and(|root| absolute.starts_with(normalize_path(root)))
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

impl FileChangeStat {
    pub fn new(path: impl Into<PathBuf>, additions: usize, deletions: usize) -> Self {
        Self {
            path: path.into(),
            additions,
            deletions,
            counts_are_projection: false,
            baseline_hash: None,
            result_hash: None,
            baseline_content: None,
            result_content: None,
        }
    }

    pub fn with_content_hashes(
        mut self,
        baseline_hash: Option<String>,
        result_hash: Option<String>,
    ) -> Self {
        self.baseline_hash = baseline_hash;
        self.result_hash = result_hash;
        self
    }

    pub fn with_content_projection(
        mut self,
        baseline_content: Option<String>,
        result_content: Option<String>,
    ) -> Self {
        self.baseline_content = bounded_content(baseline_content);
        self.result_content = bounded_content(result_content);
        self
    }

    pub fn with_count_projection(mut self, counts_are_projection: bool) -> Self {
        self.counts_are_projection = counts_are_projection;
        self
    }

    pub fn as_count_projection(self) -> Self {
        self.with_count_projection(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums_totals_across_files() {
        let summary = ChangedFilesSummary {
            files: vec![
                FileChangeStat::new("site/index.html", 10, 1),
                FileChangeStat::new("site/style.css", 5, 0),
            ],
            ..Default::default()
        };
        assert_eq!(summary.total_additions(), 15);
        assert_eq!(summary.total_deletions(), 1);
    }

    #[test]
    fn recognizes_external_and_legacy_visualization_artifacts() {
        let repo = Path::new("/projects/demo");
        let external = Path::new("/app/agents/123/artifacts/visualizations");

        assert!(is_visualization_artifact_with_root(
            repo,
            Some(external),
            Path::new("/app/agents/123/artifacts/visualizations/chart.html")
        ));
        assert!(is_visualization_artifact_with_root(
            repo,
            Some(external),
            Path::new(".codex/visualizations/2026/chart.html")
        ));
        assert!(!is_visualization_artifact_with_root(
            repo,
            Some(external),
            Path::new("src/chart.html")
        ));
    }

    #[test]
    fn ledger_keeps_exact_and_observed_attribution_separate() {
        let mut ledger = ChangedFilesSummary::default();
        ledger.merge_turn(&ChangedFilesSummary::attributed(
            "turn-a",
            vec![FileChangeStat::new("src/a.rs", 4, 1)],
            vec![
                FileChangeStat::new("generated.css", 8, 0),
                FileChangeStat::new("src/a.rs", 99, 99),
            ],
        ));

        assert_eq!(ledger.files, vec![FileChangeStat::new("src/a.rs", 4, 1)]);
        assert_eq!(
            ledger.observed_files,
            vec![FileChangeStat::new("generated.css", 8, 0)]
        );
    }

    #[test]
    fn exact_revert_clears_cumulative_entry_but_not_turn_receipts() {
        let first = ChangedFilesSummary::attributed(
            "turn-a",
            vec![FileChangeStat::new("src/a.rs", 1, 0)
                .with_content_hashes(Some("base".into()), Some("edited".into()))],
            Vec::new(),
        );
        let revert = ChangedFilesSummary::attributed(
            "turn-b",
            vec![FileChangeStat::new("src/a.rs", 0, 1)
                .with_content_hashes(Some("edited".into()), Some("base".into()))],
            Vec::new(),
        );
        let mut ledger = ChangedFilesSummary::default();
        ledger.merge_turn(&first);
        ledger.merge_turn(&revert);

        assert!(ledger.files.is_empty());
        assert_eq!(first.files.len(), 1);
        assert_eq!(revert.files.len(), 1);
    }

    #[test]
    fn partial_revert_recomputes_net_stats_from_chat_baseline() {
        let first = ChangedFilesSummary::attributed(
            "turn-a",
            vec![FileChangeStat::new("src/a.rs", 2, 2)
                .with_content_hashes(Some("base".into()), Some("two-edits".into()))
                .with_content_projection(Some("a\nb\nc\n".into()), Some("A\nb\nC\n".into()))],
            Vec::new(),
        );
        let partial_revert = ChangedFilesSummary::attributed(
            "turn-b",
            vec![FileChangeStat::new("src/a.rs", 1, 1)
                .with_content_hashes(Some("two-edits".into()), Some("one-edit".into()))
                .with_content_projection(Some("A\nb\nC\n".into()), Some("A\nb\nc\n".into()))],
            Vec::new(),
        );
        let mut ledger = ChangedFilesSummary::default();
        ledger.merge_turn(&first);
        ledger.merge_turn(&partial_revert);

        assert_eq!(ledger.files.len(), 1);
        assert_eq!(ledger.files[0].additions, 1);
        assert_eq!(ledger.files[0].deletions, 1);
        assert_eq!(
            ledger.files[0].baseline_content.as_deref(),
            Some("a\nb\nc\n")
        );
        assert_eq!(ledger.files[0].result_content.as_deref(), Some("A\nb\nc\n"));
    }

    #[test]
    fn cumulative_count_projections_replace_instead_of_accumulating() {
        let mut ledger = ChangedFilesSummary::default();
        ledger.merge_turn(&ChangedFilesSummary::attributed(
            "turn-a",
            vec![FileChangeStat::new("src/a.rs", 1, 0).as_count_projection()],
            Vec::new(),
        ));
        ledger.merge_turn(&ChangedFilesSummary::attributed(
            "turn-b",
            vec![FileChangeStat::new("src/a.rs", 2, 0).as_count_projection()],
            Vec::new(),
        ));

        assert_eq!(ledger.files[0].additions, 2);
        assert_eq!(ledger.files[0].deletions, 0);
    }

    #[test]
    fn activities_fold_into_one_turn_receipt_without_promoting_observations() {
        let activities = [
            FileChangeActivity::new(
                "edit-a",
                "turn-a",
                FileChangeStat::new("index.html", 2, 1),
                false,
                10,
            ),
            FileChangeActivity::new(
                "edit-b",
                "turn-a",
                FileChangeStat::new("index.html", 1, 0),
                false,
                11,
            ),
            FileChangeActivity::new(
                "command-a",
                "turn-a",
                FileChangeStat::new("generated.css", 8, 2).as_count_projection(),
                true,
                12,
            ),
        ];

        let summary = ChangedFilesSummary::from_activities("turn-a", &activities);

        assert_eq!(summary.files, vec![FileChangeStat::new("index.html", 3, 1)]);
        assert_eq!(summary.observed_files.len(), 1);
        assert_eq!(
            summary.observed_files[0].path,
            PathBuf::from("generated.css")
        );
        assert_eq!(summary.turn_id.as_deref(), Some("turn-a"));
    }

    #[test]
    fn empty_file_existence_changes_are_not_treated_as_content_reverts() {
        let mut created = ChangedFilesSummary::default();
        created.merge_turn(&ChangedFilesSummary::attributed(
            "create-empty",
            vec![FileChangeStat::new(".gitkeep", 0, 0)
                .with_content_hashes(Some("missing".into()), Some("empty-file-hash".into()))
                .with_content_projection(Some(String::new()), Some(String::new()))],
            Vec::new(),
        ));
        assert_eq!(created.files.len(), 1);

        let mut deleted = ChangedFilesSummary::default();
        deleted.merge_turn(&ChangedFilesSummary::attributed(
            "delete-empty",
            vec![FileChangeStat::new("empty.txt", 0, 0)
                .with_content_hashes(Some("empty-file-hash".into()), Some("missing".into()))
                .with_content_projection(Some(String::new()), Some(String::new()))],
            Vec::new(),
        ));
        assert_eq!(deleted.files.len(), 1);
    }

    #[test]
    fn concurrent_turns_do_not_leak_unattributed_paths() {
        let mut chat_a = ChangedFilesSummary::default();
        let mut chat_b = ChangedFilesSummary::default();
        chat_a.merge_turn(&ChangedFilesSummary::attributed(
            "a",
            vec![FileChangeStat::new("src/a.rs", 1, 0)],
            Vec::new(),
        ));
        chat_b.merge_turn(&ChangedFilesSummary::attributed(
            "b",
            vec![FileChangeStat::new("src/b.rs", 2, 0)],
            Vec::new(),
        ));

        assert_eq!(chat_a.files[0].path, PathBuf::from("src/a.rs"));
        assert_eq!(chat_b.files[0].path, PathBuf::from("src/b.rs"));
    }
}
