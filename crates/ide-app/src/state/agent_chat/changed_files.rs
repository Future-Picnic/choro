use std::path::{Component, Path, PathBuf};

use uuid::Uuid;

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
    pub files: Vec<FileChangeStat>,
    pub snapshot_id: Option<Uuid>,
    pub commit_sha: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileChangeStat {
    pub path: PathBuf,
    pub additions: usize,
    pub deletions: usize,
}

impl ChangedFilesSummary {
    pub fn total_additions(&self) -> usize {
        self.files.iter().map(|file| file.additions).sum()
    }

    pub fn total_deletions(&self) -> usize {
        self.files.iter().map(|file| file.deletions).sum()
    }

    pub fn remove_visualization_artifacts(&mut self, agent_id: Uuid, repo_path: &Path) {
        let filter = VisualizationArtifactFilter::new(agent_id, repo_path);
        self.files.retain(|file| !filter.is_artifact(&file.path));
    }
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
        }
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
}
