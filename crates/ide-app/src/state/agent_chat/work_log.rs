use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkLogEntry {
    pub id: String,
    pub collapse_key: String,
    pub kind: WorkLogEntryKind,
    pub title: String,
    pub detail: Option<String>,
    pub status: WorkLogStatus,
    pub started_at: u64,
    pub updated_at: u64,
    pub count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkLogEntryKind {
    Tool,
    Command,
    Step,
    Plan,
    UserInput,
    System,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkLogStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

impl WorkLogEntry {
    pub fn new(
        id: impl Into<String>,
        collapse_key: impl Into<String>,
        kind: WorkLogEntryKind,
        title: impl Into<String>,
        status: WorkLogStatus,
    ) -> Self {
        let now = unix_now();
        Self {
            id: id.into(),
            collapse_key: collapse_key.into(),
            kind,
            title: title.into(),
            detail: None,
            status,
            started_at: now,
            updated_at: now,
            count: 1,
        }
    }

    pub fn detail(mut self, detail: impl Into<Option<String>>) -> Self {
        self.detail = detail.into();
        self
    }

    /// Sanitizes externally supplied diagnostic fields before they enter the
    /// in-memory timeline or local persistence.
    pub fn redact_sensitive(mut self) -> Self {
        self.id = ide_core::redact_sensitive_text(&self.id);
        self.collapse_key = ide_core::redact_sensitive_text(&self.collapse_key);
        self.title = ide_core::redact_sensitive_text(&self.title);
        self.detail = self
            .detail
            .map(|detail| ide_core::redact_sensitive_text(&detail));
        self
    }

    pub fn merge(&mut self, next: WorkLogEntry) {
        let same_action = self.id == next.id;
        self.title = next.title;
        self.detail = next.detail.or_else(|| self.detail.clone());
        self.status = next.status;
        self.updated_at = next.updated_at;
        if !same_action {
            self.count += next.count;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_work_log_fields_are_redacted_before_persistence() {
        let entry = WorkLogEntry::new(
            "tool-token=secret-token-123",
            "Authorization: Bearer secret-token-123",
            WorkLogEntryKind::Tool,
            "curl --api-key secret-token-123",
            WorkLogStatus::Failed,
        )
        .detail(Some(
            "mongodb://user:secret-password@localhost/app".to_string(),
        ))
        .redact_sensitive();

        let debug = format!("{entry:?}");
        assert!(!debug.contains("secret-token-123"));
        assert!(!debug.contains("secret-password"));
        assert!(debug.contains("[REDACTED]"));
    }

    #[test]
    fn lifecycle_updates_do_not_count_as_additional_actions() {
        let mut entry = WorkLogEntry::new(
            "command-1",
            "command-1",
            WorkLogEntryKind::Command,
            "npm test",
            WorkLogStatus::InProgress,
        );
        entry.merge(WorkLogEntry::new(
            "command-1",
            "command-1",
            WorkLogEntryKind::Command,
            "npm test",
            WorkLogStatus::Completed,
        ));

        assert_eq!(entry.count, 1);
        assert_eq!(entry.status, WorkLogStatus::Completed);
    }

    #[test]
    fn distinct_actions_with_one_collapse_key_still_aggregate() {
        let mut entry = WorkLogEntry::new(
            "read-1",
            "exploration",
            WorkLogEntryKind::Tool,
            "Read file",
            WorkLogStatus::Completed,
        );
        entry.merge(WorkLogEntry::new(
            "read-2",
            "exploration",
            WorkLogEntryKind::Tool,
            "Read file",
            WorkLogStatus::Completed,
        ));

        assert_eq!(entry.count, 2);
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}
