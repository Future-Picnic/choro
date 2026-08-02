#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PendingApprovalKind {
    Command,
    FileChange,
    Permissions,
}

impl PendingApprovalKind {
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::Command => "command",
            Self::FileChange => "file_change",
            Self::Permissions => "permissions",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingApproval {
    pub request_id: String,
    pub kind: PendingApprovalKind,
    pub title: String,
    pub detail: Option<String>,
}

impl PendingApproval {
    pub fn new(
        request_id: impl Into<String>,
        kind: PendingApprovalKind,
        title: impl Into<String>,
        detail: Option<String>,
    ) -> Self {
        Self {
            request_id: request_id.into(),
            kind,
            title: title.into(),
            detail,
        }
    }
}
