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
        self.title = next.title;
        self.detail = next.detail.or_else(|| self.detail.clone());
        self.status = next.status;
        self.updated_at = next.updated_at;
        self.count += next.count;
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
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}
