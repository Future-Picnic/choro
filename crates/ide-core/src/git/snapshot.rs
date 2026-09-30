use std::path::PathBuf;

/// Where HEAD points right now.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HeadInfo {
    /// Branch name if on a branch, e.g. "main".
    pub branch: Option<String>,
    /// Short commit id, e.g. "a1b2c3d".
    pub oid_short: String,
    pub detached: bool,
    /// Repository has no commits yet (fresh `git init`).
    pub unborn: bool,
}

impl HeadInfo {
    pub fn display_name(&self) -> String {
        match (&self.branch, self.detached, self.unborn) {
            (Some(branch), _, _) => branch.clone(),
            (None, true, _) => format!("detached @ {}", self.oid_short),
            (None, _, true) => "no commits yet".to_string(),
            _ => self.oid_short.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchInfo {
    pub name: String,
    pub is_remote: bool,
    pub is_head: bool,
    pub upstream: Option<String>,
    pub ahead: usize,
    pub behind: usize,
    /// First line of the tip commit's message.
    pub tip_summary: String,
    /// Author name of the tip commit.
    pub tip_author: String,
    /// Tip commit time as unix seconds (0 if unknown).
    pub tip_time: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    Untracked,
    Conflicted,
}

impl ChangeKind {
    pub fn letter(&self) -> &'static str {
        match self {
            ChangeKind::Added => "A",
            ChangeKind::Modified => "M",
            ChangeKind::Deleted => "D",
            ChangeKind::Renamed => "R",
            ChangeKind::Untracked => "U",
            ChangeKind::Conflicted => "!",
        }
    }
}

/// One file in `git status`, split by index (staged) and worktree (unstaged) sides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusEntry {
    pub path: PathBuf,
    pub staged: Option<ChangeKind>,
    pub unstaged: Option<ChangeKind>,
    pub staged_stats: LineStats,
    pub unstaged_stats: LineStats,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LineStats {
    pub insertions: usize,
    pub deletions: usize,
}

/// Everything the git panel needs, produced in one background read.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GitSnapshot {
    pub head: HeadInfo,
    pub branches: Vec<BranchInfo>,
    pub entries: Vec<StatusEntry>,
    /// Lines added across all uncommitted changes (incl. untracked files).
    pub insertions: usize,
    /// Lines removed across all uncommitted changes.
    pub deletions: usize,
    /// The repo's primary remote (`origin`, else the first configured one).
    /// Cached here so render paths never have to ask git for it.
    pub primary_remote: Option<super::GitRemote>,
    /// GitHub account bound to the primary remote, if the user assigned one.
    pub assigned_account: Option<String>,
}

impl GitSnapshot {
    pub fn staged(&self) -> impl Iterator<Item = &StatusEntry> {
        self.entries.iter().filter(|e| e.staged.is_some())
    }

    pub fn unstaged(&self) -> impl Iterator<Item = &StatusEntry> {
        self.entries
            .iter()
            .filter(|e| matches!(e.unstaged, Some(k) if k != ChangeKind::Untracked))
    }

    pub fn untracked(&self) -> impl Iterator<Item = &StatusEntry> {
        self.entries
            .iter()
            .filter(|e| e.unstaged == Some(ChangeKind::Untracked))
    }
}
