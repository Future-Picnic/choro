//! Additive phone contract. Legacy integrations keep their original projections.
use serde::{Deserialize, Serialize};

pub const VERSION: u32 = 11;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct RemoteQuery {
    pub agent_id: Option<String>,
    pub query: Option<String>,
    pub cursor: Option<String>,
    pub path: Option<String>,
    pub turn_id: Option<String>,
    pub run_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AgentAction {
    pub client_command_id: String,
    pub agent_id: String,
    #[serde(flatten)]
    pub action: Action,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    Metadata {
        title: Option<String>,
        pinned: Option<bool>,
        status: Option<String>,
    },
    Mode {
        mode: String,
    },
    Checklist {
        checklist_id: String,
        item_id: String,
        checked: bool,
        expected_checked: bool,
    },
    ChecklistRetry {
        source_turn_id: String,
    },
    ReviewFix {
        review_id: String,
        revision: String,
        finding_ids: Vec<String>,
    },
    Delegation {
        run_id: String,
        task_id: Option<String>,
        revision: u64,
        operation: String,
    },
    Summary,
}

pub fn capabilities(permission: super::DevicePermission) -> serde_json::Value {
    serde_json::json!({
        "protocol_version": VERSION,
        "permission": permission,
        "features": ["agent_workspace", "agent_search", "rich_timeline", "image_attachments", "band", "review_actions", "receipt_diffs", "durable_commands"],
        "limits": {"images_per_message": 4, "image_bytes": 4 * 1024 * 1024, "chunk_bytes": 256 * 1024},
        "can_control": permission != super::DevicePermission::ViewOnly,
        "can_approve": permission == super::DevicePermission::FullAccess,
        "can_ship": permission == super::DevicePermission::FullAccess
    })
}
