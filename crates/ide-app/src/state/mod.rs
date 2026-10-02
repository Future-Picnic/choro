pub mod agent_activity;
pub mod agent_navigation;
pub mod agent_capabilities;
pub mod agent_chat;
pub mod agents;
pub mod chat_dispatch;
pub mod delegation;
pub mod designs;
pub mod doc_assistant;
pub mod docs;
pub mod git_state;
pub mod open_code;
pub mod orbit;
pub mod quick_ask;
pub mod services;
pub(crate) mod sidebar;
pub(crate) mod subscription_usage;
mod snapshot_writer;
pub mod tasks;
pub mod terminals;
pub mod workspace;

pub use agent_activity::AgentActivityCache;
pub use agent_capabilities::{
    AgentCapability, AgentCapabilityCacheFile, AgentCapabilitySource, ChoroRiff, ChoroRiffStore,
    CHORO_RIFFS_SCHEMA_VERSION,
};
pub use agent_chat::AgentChatState;
pub use agents::AgentRecords;
pub use designs::DesignsState;
pub use doc_assistant::DocAssistantState;
pub use docs::{DocSaveStatus, DocsState};
pub use git_state::{GitState, GitStates};
pub use open_code::{OpenCodeCatalog, OpenCodeCatalogState, OpenCodeModel};
pub use orbit::{OrbitEvent, OrbitState};
pub use quick_ask::{QuickAskEvent, QuickAskPhase, QuickAskScope, QuickAskState};
pub use services::{ServicesScanKind, ServicesState};
pub use tasks::TasksState;
pub use terminals::{SessionId, TerminalManager};
pub use workspace::Workspace;
#[allow(unused_imports)]
pub use workspace::WorkspaceEvent;

fn preferred_selection<T: Clone + PartialEq>(selected: Option<&T>, available: &[T]) -> Option<T> {
    selected
        .filter(|selected| available.contains(selected))
        .cloned()
        .or_else(|| available.first().cloned())
}
