use std::path::PathBuf;
use std::time::{Duration, Instant};

pub(crate) use super::agent_chat::protocol::catalog::discover;
pub use ide_core::agents::codex::CodexModel;

#[derive(Clone, Debug, Default)]
pub struct CodexCatalog {
    pub loading: bool,
    pub project_path: Option<PathBuf>,
    pub models: Vec<CodexModel>,
    pub error: Option<String>,
    pub executable: Option<PathBuf>,
    pub version: Option<(u32, u32, u32)>,
    pub refreshed_at: Option<Instant>,
}

impl CodexCatalog {
    pub fn stale_for(&self, path: &std::path::Path) -> bool {
        self.project_path.as_deref() != Some(path)
            || self
                .refreshed_at
                .is_none_or(|at| at.elapsed() > Duration::from_secs(30))
    }
}
