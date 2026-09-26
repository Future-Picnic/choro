//! Local, revisioned HTML design documents. Every writer (UI or MCP) uses this service.
mod settings;
pub use settings::*;
mod activity;
mod agent;
mod canvas;
pub use activity::*;
pub use canvas::*;
mod store;
pub use agent::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
pub use store::*;
use uuid::Uuid;

pub const DESIGNS_DIR: &str = "choro_designs";
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct StudioScreen {
    pub id: Uuid,
    pub name: String,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub files: StudioScreenFiles,
}
/// Filenames relative to this screen's identity-based directory.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct StudioScreenFiles {
    pub html: String,
    pub css: String,
    pub js: String,
}
impl Default for StudioScreenFiles {
    fn default() -> Self {
        Self {
            html: "index.html".into(),
            css: "styles.css".into(),
            js: "prototype.js".into(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct StudioDesignManifest {
    pub schema_version: u32,
    pub id: Uuid,
    pub name: String,
    pub revision: u64,
    pub design_system: String,
    #[serde(default)]
    pub system_workspace: bool,
    pub screens: Vec<StudioScreen>,
    pub source_doc: Option<String>,
    pub source_task: Option<String>,
    #[serde(default)]
    pub source_context: BTreeMap<String, StudioSource>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct StudioDocument {
    pub html: String,
    pub css: String,
    pub js: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct StudioDesignSystem {
    pub schema_version: u32,
    pub revision: u64,
    pub tokens: BTreeMap<String, String>,
    pub recipes: BTreeMap<String, BTreeMap<String, String>>,
    #[serde(default)]
    pub font_faces: Vec<StudioFontFace>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct StudioFontFace {
    pub family: String,
    pub file: String,
    pub weight: u16,
    #[serde(default)]
    pub italic: bool,
}
impl Default for StudioDesignSystem {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            revision: 0,
            font_faces: Vec::new(),
            tokens: [
                ("color-background", "#ffffff"),
                ("color-surface", "#f4f5f7"),
                ("color-text", "#202124"),
                ("color-muted", "#60646c"),
                ("color-primary", "#335cff"),
                ("color-on-primary", "#ffffff"),
                ("font-body", "system-ui, sans-serif"),
                ("font-size-body", "16px"),
                ("font-size-heading", "32px"),
                ("space-small", "8px"),
                ("space-medium", "16px"),
                ("space-large", "32px"),
                ("radius-control", "6px"),
                ("radius-card", "12px"),
                ("shadow-card", "0 2px 8px #00000012"),
            ]
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect(),
            recipes: [
                (
                    "button",
                    [
                        ("background", "var(--color-primary)"),
                        ("color", "var(--color-on-primary)"),
                        ("border-radius", "var(--radius-control)"),
                    ]
                    .as_slice(),
                ),
                (
                    "input",
                    [
                        ("background", "var(--color-background)"),
                        ("color", "var(--color-text)"),
                        ("border-radius", "var(--radius-control)"),
                    ]
                    .as_slice(),
                ),
                (
                    "card",
                    [
                        ("background", "var(--color-surface)"),
                        ("padding", "var(--space-medium)"),
                        ("border-radius", "var(--radius-card)"),
                    ]
                    .as_slice(),
                ),
                (
                    "heading",
                    [
                        ("font-size", "var(--font-size-heading)"),
                        ("color", "var(--color-text)"),
                        ("font-family", "var(--font-body)"),
                    ]
                    .as_slice(),
                ),
            ]
            .into_iter()
            .map(|(k, v)| {
                (
                    k.into(),
                    v.iter()
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                        .collect(),
                )
            })
            .collect(),
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct StudioOverrides {
    pub tokens: BTreeMap<String, String>,
    /// File-backed local CSS and inline/style-element overrides, relative to the design.
    /// Maintained by the transaction service for every writer, never agent-controlled paths.
    #[serde(default)]
    pub screen_styles: BTreeMap<Uuid, StudioStyleFiles>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct StudioStyleFiles {
    pub stylesheet: String,
    pub inline_styles: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StudioSavedRevision {
    pub design: StudioDesign,
    pub assets: BTreeMap<String, Vec<u8>>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct StudioSource {
    pub reference: String,
    pub content: String,
    /// Stable tracker identity for reciprocal task links, even before its board loads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_ref: Option<crate::TaskRef>,
}

impl StudioDesignManifest {
    pub fn linked_task(&self) -> Option<&crate::TaskRef> {
        self.source_context.get("task")?.task_ref.as_ref()
    }

    pub fn links_task(&self, task: &crate::TaskRef) -> bool {
        if let Some(source) = self.linked_task() {
            return source.same_issue(task);
        }
        self.source_task
            .as_ref()
            .is_some_and(|source| source == &format!("{} {}", task.issue_key, task.issue_url))
    }

    pub fn links_doc(&self, path: &std::path::Path) -> bool {
        self.source_doc
            .as_deref()
            .is_some_and(|source| std::path::Path::new(source) == path)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct StudioDesign {
    pub manifest: StudioDesignManifest,
    pub documents: BTreeMap<Uuid, StudioDocument>,
    pub system: StudioDesignSystem,
    pub overrides: StudioOverrides,
    /// Includes external edits, tokens, and assets, not only the manifest counter.
    pub fingerprint: String,
    #[serde(default)]
    pub asset_fingerprint: String,
}
impl StudioDesign {
    pub fn tokens(&self) -> BTreeMap<String, String> {
        let mut tokens = self.system.tokens.clone();
        tokens.extend(self.overrides.tokens.clone());
        tokens
    }
    pub fn tokens_css(&self) -> String {
        let mut css = String::from(":root{");
        for (name, value) in self.tokens() {
            css.push_str(&format!("--{name}:{value};"));
        }
        css.push('}');
        for (name, properties) in &self.system.recipes {
            css.push_str(&format!(".ds-{name}{{"));
            for (key, value) in properties {
                css.push_str(&format!("{key}:{value};"));
            }
            css.push('}');
        }
        for font in &self.system.font_faces {
            css.push_str(&format!("@font-face{{font-family:'{}';src:url('design-system/assets/{}');font-weight:{};font-style:{};font-display:swap;}}",font.family,font.file,font.weight,if font.italic {"italic"} else {"normal"}));
        }
        css
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct StudioAgentContext {
    #[serde(default)]
    pub target: StudioAgentTarget,
    pub design_id: Uuid,
    pub conversation_id: Uuid,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StudioAgentTarget {
    #[default]
    Design,
    DesignSystem,
    DesignSystemImport,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct StudioTurnScope {
    #[serde(default)]
    pub design_guidance: String,
    pub id: Uuid,
    pub design_id: Uuid,
    pub screen_ids: BTreeSet<Uuid>,
    pub selected_element: Option<String>,
    #[serde(default)]
    pub current_screen_id: Option<Uuid>,
    #[serde(default)]
    pub base_revision: u64,
    #[serde(default)]
    pub base_fingerprint: String,
    pub allow_create: bool,
    pub allow_design_overrides: bool,
    #[serde(default)]
    pub allow_design_metadata: bool,
    pub allow_shared_system: bool,
    #[serde(default)]
    pub allow_system_binding: bool,
    pub active: bool,
    #[serde(default)]
    pub expires_at: u64,
}
impl StudioTurnScope {
    pub fn screen(design_id: Uuid, screen_id: Uuid) -> Self {
        Self {
            id: Uuid::new_v4(),
            design_id,
            design_guidance: String::new(),
            screen_ids: [screen_id].into(),
            selected_element: None,
            current_screen_id: Some(screen_id),
            base_revision: 0,
            base_fingerprint: String::new(),
            allow_create: false,
            allow_design_overrides: false,
            allow_design_metadata: false,
            allow_shared_system: false,
            allow_system_binding: false,
            active: true,
            expires_at: now() + 1800,
        }
    }
    pub fn whole_design(design: &StudioDesign) -> Self {
        Self {
            screen_ids: design.manifest.screens.iter().map(|s| s.id).collect(),
            allow_create: true,
            allow_design_overrides: true,
            allow_design_metadata: true,
            ..Self::screen(design.manifest.id, Uuid::nil())
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum StudioOperation {
    AddAsset {
        name: String,
        bytes: Vec<u8>,
    },
    WriteScreen {
        screen_id: Uuid,
        document: StudioDocument,
    },
    CreateScreen {
        screen: StudioScreen,
        document: StudioDocument,
    },
    UpdateScreen {
        screen: StudioScreen,
    },
    Reorder {
        screen_ids: Vec<Uuid>,
    },
    SetSource {
        document: Option<StudioSource>,
        task: Option<StudioSource>,
    },
    RenameDesign {
        name: String,
    },
    SetOverrides {
        overrides: StudioOverrides,
    },
    BindSystem {
        system_id: Option<Uuid>,
        expected_system_fingerprint: Option<String>,
    },
    SystemDetails {
        platform: String,
        sources: BTreeMap<String, String>,
    },
    SetSystem {
        system: StudioDesignSystem,
        expected_system_revision: u64,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StudioTransaction {
    pub id: Uuid,
    pub scope_id: Uuid,
    pub design_id: Uuid,
    pub expected_revision: u64,
    pub expected_fingerprint: String,
    pub operations: Vec<StudioOperation>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StudioHandoff {
    #[serde(default)]
    pub design_system_context: serde_json::Value,
    pub schema_version: u32,
    pub id: Uuid,
    pub design: StudioDesign,
    pub screen_ids: Vec<Uuid>,
    /// Relative asset paths -> bytes; carried with the snapshot, including untracked files.
    pub assets: BTreeMap<String, Vec<u8>>,
    pub thumbnails: BTreeMap<Uuid, Vec<u8>>,
    pub instruction: String,
}

pub fn starter_document() -> StudioDocument {
    StudioDocument {
        html: "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"></head><body><main data-studio-id=\"main\"><h1 data-studio-id=\"title\" class=\"ds-heading\">Your next screen</h1><p data-studio-id=\"description\">Describe what you want to design in the Agent sidebar.</p><button data-studio-id=\"action\" class=\"ds-button\">Continue</button></main></body></html>".into(),
        css: "body{margin:0;background:var(--color-background, white);color:var(--color-text, #202124);font-family:var(--font-body, system-ui);font-size:var(--font-size-body, 16px)}main{padding:var(--space-large, 32px);max-width:960px;margin:auto}p{color:var(--color-muted, #60646c);line-height:1.6}button{border:0;padding:var(--space-small, 8px) var(--space-medium, 16px);font:inherit;cursor:pointer}".into(),
        js: String::new(),
    }
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub use agent::{codebase_context_instruction, preview_review_prompt};
