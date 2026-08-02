use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{anyhow, Context as _, Result};
use gpui::{App, AppContext, Context, Entity, EventEmitter};
use ide_core::local_store::{
    LocalStore, StoredPenpotConnection, StoredPenpotDesign, StoredPenpotDesignConversation,
    StoredProjectPenpotBinding,
};
use ide_core::{AgentRecord, ProjectId, TaskRef};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

const DEFAULT_INSTANCE_URL: &str = "https://82-70-214-124.sslip.io";
const DEFAULT_MCP_URL: &str = "https://82-70-214-124.sslip.io/mcp/stream";
const DEFAULT_PROVISIONING_URL: &str = "https://82-70-214-124.sslip.io/choro/penpot/v1/bootstrap";
const DEFAULT_PENPOT_KEYCHAIN_SERVICE: &str = "com.ritmus.choro.penpot.mcp";
const DEFAULT_INSTALLATION_KEYCHAIN_SERVICE: &str = "com.ritmus.choro.installation";
const INSTALLATION_ID_ACCOUNT: &str = "installation-id";
const INSTALLATION_SECRET_ACCOUNT: &str = "installation-secret";
const LEGACY_KEYCHAIN_ACCOUNT: &str = "default";
const LEGACY_KEYCHAIN_ACCESS_TOKEN_ACCOUNT: &str = "access-token";
const NO_WORKSPACE_ERROR: &str =
    "No Design workspace is available for this account. Create a Design project and try again.";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct LegacyPenpotDesign {
    pub id: Uuid,
    pub penpot_project_id: Uuid,
    pub team_id: Uuid,
    pub name: String,
    #[serde(default)]
    pub page_id: Option<Uuid>,
}

pub type PenpotDesign = StoredPenpotDesign;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PenpotConfig {
    #[serde(default = "default_instance_url")]
    pub instance_url: String,
    #[serde(default = "default_mcp_url")]
    pub mcp_url: String,
    #[serde(default)]
    designs: HashMap<ProjectId, Vec<LegacyPenpotDesign>>,
    #[serde(default)]
    pub selected_designs: HashMap<ProjectId, Uuid>,
}

impl Default for PenpotConfig {
    fn default() -> Self {
        Self {
            instance_url: default_instance_url(),
            mcp_url: default_mcp_url(),
            designs: HashMap::new(),
            selected_designs: HashMap::new(),
        }
    }
}

impl PenpotConfig {
    fn path() -> PathBuf {
        ide_core::AppConfig::config_root().join("penpot.json")
    }

    fn load() -> Self {
        fs::read_to_string(Self::path())
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn normalized(instance_url: &str, mcp_url: &str) -> Result<(Self, Option<String>)> {
        let instance_url = normalize_http_url(instance_url, "Design service URL")?;
        let mut parsed = Url::parse(mcp_url.trim()).context("MCP URL is not a valid URL")?;
        anyhow::ensure!(
            matches!(parsed.scheme(), "http" | "https"),
            "MCP URL must use http or https"
        );
        let embedded_key = parsed
            .query_pairs()
            .find(|(name, _)| name == "userToken")
            .map(|(_, value)| value.into_owned());
        if embedded_key.is_some() {
            let retained = parsed
                .query_pairs()
                .filter(|(name, _)| name != "userToken")
                .map(|(name, value)| (name.into_owned(), value.into_owned()))
                .collect::<Vec<_>>();
            parsed.set_query(None);
            if !retained.is_empty() {
                parsed.query_pairs_mut().extend_pairs(retained);
            }
        }
        Ok((
            Self {
                instance_url,
                mcp_url: parsed.to_string(),
                designs: HashMap::new(),
                selected_designs: HashMap::new(),
            },
            embedded_key,
        ))
    }

    pub fn is_local(&self) -> bool {
        Url::parse(&self.mcp_url)
            .ok()
            .is_some_and(|url| matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1")))
    }

    pub fn full_mcp_url(&self, key: Option<&str>) -> Result<String> {
        let mut url = Url::parse(&self.mcp_url).context("stored MCP URL is invalid")?;
        if !self.is_local() {
            let key = key
                .map(str::trim)
                .filter(|key| !key.is_empty())
                .context("Design MCP key is missing")?;
            url.query_pairs_mut().append_pair("userToken", key);
        }
        Ok(url.to_string())
    }

    fn workspace_url(&self, design: &PenpotDesign) -> Result<String> {
        let mut url =
            Url::parse(&self.instance_url).context("stored Design service URL is invalid")?;
        let mut query = format!(
            "team-id={}&file-id={}",
            design.penpot_team_id, design.penpot_file_id
        );
        if let Some(page_id) = design.page_id {
            query.push_str(&format!("&page-id={page_id}"));
        }
        url.set_fragment(Some(&format!("/workspace?{query}")));
        Ok(url.to_string())
    }

    fn external_mcp_workspace_url(&self, design: &PenpotDesign) -> Result<String> {
        let mut url = Url::parse(&self.workspace_url(design)?)
            .context("stored Design workspace URL is invalid")?;
        let fragment = url
            .fragment()
            .context("stored Design workspace URL has no route")?;
        let separator = if fragment.contains('?') { '&' } else { '?' };
        let fragment = format!("{fragment}{separator}choro-mcp=connect");
        url.set_fragment(Some(&fragment));
        Ok(url.to_string())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PenpotConnectionStatus {
    NotChecked,
    Provisioning,
    Checking,
    Reachable,
    Error(String),
}

pub enum PenpotEvent {
    Changed,
    DesignCreated {
        project: ProjectId,
        design_id: Uuid,
        initial_prompt: Option<String>,
    },
}

#[derive(Clone, Debug)]
pub enum PenpotDesignSource {
    Document(PathBuf),
    Task(TaskRef),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DesignProvider {
    #[default]
    Choro,
    PenpotCloud,
}

impl DesignProvider {
    pub fn label(self) -> &'static str {
        match self {
            Self::Choro => "Choro Design",
            Self::PenpotCloud => "Penpot Cloud",
        }
    }
}

pub struct PenpotState {
    config: PenpotConfig,
    connection: Option<StoredPenpotConnection>,
    designs: HashMap<ProjectId, Vec<PenpotDesign>>,
    selected_designs: HashMap<ProjectId, Uuid>,
    thumbnail_paths: HashMap<Uuid, PathBuf>,
    thumbnail_refreshing: HashSet<ProjectId>,
    conversations: HashMap<Uuid, Vec<StoredPenpotDesignConversation>>,
    assistant_busy: HashMap<ProjectId, bool>,
    has_key: bool,
    has_access_token: bool,
    status: PenpotConnectionStatus,
    creating_design: bool,
    last_error: Option<String>,
}

impl EventEmitter<PenpotEvent> for PenpotState {}

impl PenpotState {
    pub fn view(cx: &mut App) -> Entity<Self> {
        cx.new(|_| {
            let legacy = PenpotConfig::load();
            let store = LocalStore::open_default().ok();
            let mut connection = store
                .as_ref()
                .and_then(|store| store.active_penpot_connection().ok())
                .flatten();
            if connection.is_none()
                && (PenpotConfig::path().is_file()
                    || load_keychain_secret(LEGACY_KEYCHAIN_ACCOUNT).is_some()
                    || load_keychain_secret(LEGACY_KEYCHAIN_ACCESS_TOKEN_ACCOUNT).is_some())
            {
                let now = ide_core::agents::unix_now();
                let migrated = StoredPenpotConnection {
                    id: Uuid::new_v4(),
                    instance_url: legacy.instance_url.clone(),
                    mcp_url: legacy.mcp_url.clone(),
                    profile_id: None,
                    profile_email: None,
                    default_team_id: None,
                    default_project_id: None,
                    is_active: true,
                    verified_at: None,
                    created_at: now,
                    updated_at: now,
                };
                if let Some(store) = store.as_ref() {
                    if store.save_active_penpot_connection(&migrated).is_ok() {
                        migrate_legacy_credentials(migrated.id);
                        migrate_legacy_designs(store, &legacy, &migrated);
                        connection = Some(migrated);
                    }
                }
            }
            let config = connection
                .as_ref()
                .map(|value| PenpotConfig {
                    instance_url: value.instance_url.clone(),
                    mcp_url: value.mcp_url.clone(),
                    designs: HashMap::new(),
                    selected_designs: HashMap::new(),
                })
                .unwrap_or(legacy);
            let active_connection_id = connection.as_ref().map(|value| value.id);
            let mut designs: HashMap<ProjectId, Vec<PenpotDesign>> = HashMap::new();
            for design in store
                .as_ref()
                .and_then(|store| store.load_all_penpot_designs().ok())
                .unwrap_or_default()
                .into_iter()
                .filter(|design| Some(design.connection_id) == active_connection_id)
            {
                designs.entry(design.project_id).or_default().push(design);
            }
            let mut selected_designs = HashMap::new();
            for project in designs.keys().copied() {
                if let Some(selected) = store
                    .as_ref()
                    .and_then(|store| {
                        store
                            .project_penpot_binding(project, active_connection_id?)
                            .ok()
                    })
                    .flatten()
                    .filter(|binding| Some(binding.connection_id) == active_connection_id)
                    .and_then(|binding| binding.selected_design_id)
                {
                    selected_designs.insert(project, selected);
                }
            }
            let conversations = store
                .as_ref()
                .map(|store| load_conversations(store, &designs))
                .unwrap_or_default();
            let has_key = connection
                .as_ref()
                .is_some_and(|value| load_mcp_key(value.id).is_some())
                || config.is_local();
            let has_access_token = connection
                .as_ref()
                .is_some_and(|value| load_access_token(value.id).is_some());
            Self {
                config,
                connection,
                designs,
                selected_designs,
                thumbnail_paths: HashMap::new(),
                thumbnail_refreshing: HashSet::new(),
                conversations,
                assistant_busy: HashMap::new(),
                has_key,
                has_access_token,
                status: PenpotConnectionStatus::NotChecked,
                creating_design: false,
                last_error: None,
            }
        })
    }

    pub fn config(&self) -> &PenpotConfig {
        &self.config
    }

    pub fn has_key(&self) -> bool {
        self.has_key
    }

    pub fn has_access_token(&self) -> bool {
        self.has_access_token
    }

    pub fn is_configured(&self) -> bool {
        self.has_key && self.has_access_token && !self.config.instance_url.trim().is_empty()
    }

    pub fn provider(&self) -> DesignProvider {
        design_provider_for_connection(self.connection.as_ref())
    }

    pub fn connection_pending(&self) -> bool {
        matches!(
            self.status,
            PenpotConnectionStatus::Provisioning | PenpotConnectionStatus::Checking
        )
    }

    pub fn connection_needs_attention(&self) -> bool {
        !self.connection_pending()
            && (!self.is_configured()
                || matches!(
                    self.status,
                    PenpotConnectionStatus::NotChecked | PenpotConnectionStatus::Error(_)
                ))
    }

    pub fn ensure_auto_provisioned(&mut self, cx: &mut Context<Self>) {
        if self.connection_pending() || self.managed_connection_ready() {
            return;
        }
        // A configured non-managed connection is an explicit user choice.
        // Never replace it merely because it uses the legacy cloud origin.
        if !should_auto_provision_connection(self.connection.as_ref()) {
            return;
        }
        self.begin_managed_provisioning(cx);
    }

    pub fn switch_to_managed(&mut self, cx: &mut Context<Self>) {
        if self.connection_pending() || self.managed_connection_ready() {
            return;
        }
        self.begin_managed_provisioning(cx);
    }

    fn begin_managed_provisioning(&mut self, cx: &mut Context<Self>) {
        self.status = PenpotConnectionStatus::Provisioning;
        self.last_error = None;
        cx.emit(PenpotEvent::Changed);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { bootstrap_managed_penpot() })
                .await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(provisioned) => {
                        if let Err(error) = this.apply_managed_provisioning(provisioned) {
                            let message =
                                redact_penpot_error(&error.to_string(), std::iter::empty::<&str>());
                            this.status = PenpotConnectionStatus::Error(message.clone());
                            this.last_error = Some(message);
                        } else {
                            this.status = PenpotConnectionStatus::Reachable;
                            this.last_error = None;
                        }
                    }
                    Err(error) => {
                        let message = redact_penpot_error(&error, std::iter::empty::<&str>());
                        this.status = PenpotConnectionStatus::Error(message.clone());
                        this.last_error = Some(message);
                    }
                }
                cx.emit(PenpotEvent::Changed);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn managed_connection_ready(&self) -> bool {
        let Some(connection) = self.connection.as_ref() else {
            return false;
        };
        self.is_configured()
            && instance_matches_managed_service(&connection.instance_url)
            && load_session_token(connection.id).is_some()
    }

    fn apply_managed_provisioning(&mut self, provisioned: ManagedPenpotBootstrap) -> Result<()> {
        let (config, _) =
            PenpotConfig::normalized(&provisioned.instance_url, &provisioned.mcp_url)?;
        let store = LocalStore::open_default()?;
        let now = ide_core::agents::unix_now();
        let existing = store
            .penpot_connection_for_profile(&config.instance_url, provisioned.profile_id)?
            .or_else(|| {
                self.connection
                    .clone()
                    .filter(|connection| connection.instance_url == config.instance_url)
            });
        let connection_id = existing
            .as_ref()
            .map(|connection| connection.id)
            .unwrap_or_else(Uuid::new_v4);
        save_mcp_key(connection_id, &provisioned.mcp_key)?;
        save_access_token(connection_id, &provisioned.api_token)?;
        save_session_token(connection_id, &provisioned.session_token)?;
        let connection = StoredPenpotConnection {
            id: connection_id,
            instance_url: config.instance_url.clone(),
            mcp_url: config.mcp_url.clone(),
            profile_id: Some(provisioned.profile_id),
            profile_email: Some(provisioned.profile_email),
            default_team_id: Some(provisioned.team_id),
            default_project_id: Some(provisioned.project_id),
            is_active: true,
            verified_at: Some(now),
            created_at: existing
                .as_ref()
                .map(|connection| connection.created_at)
                .unwrap_or(now),
            updated_at: now,
        };
        store.save_active_penpot_connection(&connection)?;
        if let (Some(project), Some(starter)) = (
            managed_starter_project_id(),
            provisioned.starter_design.as_ref(),
        ) {
            persist_managed_starter_design(&store, project, connection_id, starter, now)?;
        }
        self.config = config;
        self.connection = Some(connection);
        self.designs = store
            .load_all_penpot_designs()?
            .into_iter()
            .filter(|design| design.connection_id == connection_id)
            .fold(HashMap::new(), |mut designs, design| {
                designs.entry(design.project_id).or_default().push(design);
                designs
            });
        self.conversations = load_conversations(&store, &self.designs);
        self.selected_designs.clear();
        for project in self.designs.keys().copied() {
            if let Some(selected) = store
                .project_penpot_binding(project, connection_id)?
                .and_then(|binding| binding.selected_design_id)
            {
                self.selected_designs.insert(project, selected);
            }
        }
        self.has_key = true;
        self.has_access_token = true;
        Ok(())
    }

    pub fn status(&self) -> &PenpotConnectionStatus {
        &self.status
    }

    pub fn designs_for_project(&self, project: ProjectId) -> Vec<PenpotDesign> {
        self.designs.get(&project).cloned().unwrap_or_default()
    }

    pub fn thumbnail_path(&self, design_id: Uuid) -> Option<PathBuf> {
        self.thumbnail_paths
            .get(&design_id)
            .filter(|path| path.is_file())
            .cloned()
    }

    /// Refresh the hub previews from Penpot's persisted thumbnail objects.
    ///
    /// The access token is used only for the authenticated fetch. The hub keeps
    /// a local display cache, never an authenticated URL or duplicate DB blob.
    pub fn refresh_design_thumbnails(&mut self, project: ProjectId, cx: &mut Context<Self>) {
        if self.thumbnail_refreshing.contains(&project) {
            return;
        }
        let Some(connection) = self.connection.clone() else {
            return;
        };
        let Some(access_token) = load_access_token(connection.id) else {
            return;
        };
        let designs = self.designs_for_project(project);
        if designs.is_empty() {
            return;
        }
        let design_ids = designs.iter().map(|design| design.id).collect::<Vec<_>>();
        let instance_url = self.config.instance_url.clone();
        self.thumbnail_refreshing.insert(project);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    fetch_penpot_design_thumbnails(&instance_url, &access_token, &designs)
                })
                .await;
            this.update(cx, |this, cx| {
                this.thumbnail_refreshing.remove(&project);
                if let Ok(thumbnails) = result {
                    for design_id in design_ids {
                        this.thumbnail_paths.remove(&design_id);
                    }
                    this.thumbnail_paths.extend(thumbnails);
                }
                cx.emit(PenpotEvent::Changed);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn design(&self, project: ProjectId, design_id: Uuid) -> Option<PenpotDesign> {
        self.designs
            .get(&project)?
            .iter()
            .find(|design| design.id == design_id)
            .cloned()
    }

    pub fn designs_for_doc(
        &self,
        project: ProjectId,
        relative_doc: &std::path::Path,
    ) -> Vec<PenpotDesign> {
        self.designs
            .get(&project)
            .into_iter()
            .flatten()
            .filter(|design| design.source_doc.as_deref() == Some(relative_doc))
            .cloned()
            .collect()
    }

    pub fn designs_for_task(&self, project: ProjectId, reference: &TaskRef) -> Vec<PenpotDesign> {
        self.designs
            .get(&project)
            .into_iter()
            .flatten()
            .filter(|design| {
                design
                    .source_task
                    .as_ref()
                    .is_some_and(|task| task.same_issue(reference))
            })
            .cloned()
            .collect()
    }

    pub fn designs_for_agent(&self, agent: &AgentRecord) -> Vec<PenpotDesign> {
        self.designs
            .get(&agent.project_id)
            .into_iter()
            .flatten()
            .filter(|design| {
                let direct_design_marker = format!("local-id=\"{}\"", design.id);
                let directly_linked = agent.doc.contains("<choro-penpot-design")
                    && agent.doc.contains(&direct_design_marker);
                let doc_matches = design.source_doc.as_ref().is_some_and(|source| {
                    agent.source_doc.as_ref() == Some(source) || agent.linked_docs.contains(source)
                });
                let task_matches = design.source_task.as_ref().is_some_and(|source| {
                    agent
                        .source_task
                        .as_ref()
                        .is_some_and(|task| task.same_issue(source))
                        || agent
                            .linked_tasks
                            .iter()
                            .any(|task| task.same_issue(source))
                });
                directly_linked || doc_matches || task_matches
            })
            .cloned()
            .collect()
    }

    pub fn move_doc_reference(
        &mut self,
        project: ProjectId,
        from: &std::path::Path,
        to: PathBuf,
        cx: &mut Context<Self>,
    ) {
        let Some(designs) = self.designs.get_mut(&project) else {
            return;
        };
        let Ok(store) = LocalStore::open_default() else {
            return;
        };
        let mut changed = false;
        for design in designs {
            if design.source_doc.as_deref() != Some(from) {
                continue;
            }
            design.source_doc = Some(to.clone());
            design.updated_at = ide_core::agents::unix_now();
            if let Err(error) = store.upsert_penpot_design(design) {
                self.last_error = Some(error.to_string());
            } else {
                changed = true;
            }
        }
        if changed {
            cx.emit(PenpotEvent::Changed);
            cx.notify();
        }
    }

    pub fn selected_design(&self, project: ProjectId) -> Option<PenpotDesign> {
        let selected = self.selected_designs.get(&project)?;
        self.designs
            .get(&project)?
            .iter()
            .find(|design| design.id == *selected)
            .cloned()
    }

    pub fn selected_design_url(&self, project: ProjectId) -> Option<String> {
        self.selected_design(project)
            .and_then(|design| self.config.workspace_url(&design).ok())
    }

    pub fn selected_design_web_url(&self, project: ProjectId) -> Option<String> {
        let target = self.selected_design_url(project)?;
        self.authenticated_web_url(&target).or(Some(target))
    }

    pub fn design_url(&self, design: &PenpotDesign) -> Option<String> {
        (self.connection.as_ref().map(|value| value.id) == Some(design.connection_id))
            .then(|| self.config.workspace_url(design).ok())
            .flatten()
    }

    /// Opens the exact design in the user's normal browser and asks the
    /// Choro-managed Design frontend to own the live MCP plugin connection.
    /// Authentication remains in the same-origin session redirect; agent
    /// prompts continue to receive the ordinary, credential-free design URL.
    pub fn external_mcp_design_url(&self, design: &PenpotDesign) -> Option<String> {
        if self.connection.as_ref().map(|value| value.id) != Some(design.connection_id) {
            return None;
        }
        let target = self.config.external_mcp_workspace_url(design).ok()?;
        self.authenticated_web_url(&target).or(Some(target))
    }

    fn authenticated_web_url(&self, target: &str) -> Option<String> {
        let connection = self.connection.as_ref()?;
        let session_token = load_session_token(connection.id)?;
        let installation = load_or_create_installation_identity().ok()?;
        let mut url = Url::parse(&self.config.instance_url).ok()?;
        url.set_path(&format!("/choro/penpot/v1/session/{}", installation.id));
        url.set_query(None);
        url.set_fragment(None);
        url.query_pairs_mut()
            .append_pair("token", &session_token)
            .append_pair("redirect", target);
        Some(url.to_string())
    }

    pub fn current_conversation(
        &self,
        project: ProjectId,
    ) -> Option<StoredPenpotDesignConversation> {
        let design = self.selected_design(project)?;
        self.conversations
            .get(&design.id)?
            .iter()
            .find(|conversation| conversation.is_current)
            .cloned()
    }

    pub fn conversations_for_selected_design(
        &self,
        project: ProjectId,
    ) -> Vec<StoredPenpotDesignConversation> {
        let Some(design) = self.selected_design(project) else {
            return Vec::new();
        };
        self.conversations
            .get(&design.id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn refresh_conversations(&mut self, design_id: Uuid, cx: &mut Context<Self>) -> Result<()> {
        let store = LocalStore::open_default()?;
        let current = store.ensure_current_penpot_conversation(design_id)?;
        let mut conversations = store.load_penpot_conversations(design_id)?;
        if !conversations
            .iter()
            .any(|conversation| conversation.id == current.id)
        {
            conversations.push(current);
        }
        self.conversations.insert(design_id, conversations);
        self.last_error = None;
        cx.emit(PenpotEvent::Changed);
        cx.notify();
        Ok(())
    }

    pub fn creating_design(&self) -> bool {
        self.creating_design
    }

    pub fn assistant_busy(&self, project: ProjectId) -> bool {
        self.assistant_busy.get(&project).copied().unwrap_or(false)
    }

    pub fn set_assistant_busy(&mut self, project: ProjectId, busy: bool, cx: &mut Context<Self>) {
        if self.assistant_busy(project) == busy {
            return;
        }
        self.assistant_busy.insert(project, busy);
        cx.emit(PenpotEvent::Changed);
        cx.notify();
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    pub fn select_design(&mut self, project: ProjectId, design_id: Uuid, cx: &mut Context<Self>) {
        let exists = self
            .designs
            .get(&project)
            .is_some_and(|designs| designs.iter().any(|design| design.id == design_id));
        if !exists {
            return;
        }
        self.selected_designs.insert(project, design_id);
        if let Err(error) = LocalStore::open_default()
            .and_then(|store| store.select_penpot_design(project, design_id))
        {
            self.last_error = Some(error.to_string());
        }
        cx.emit(PenpotEvent::Changed);
        cx.notify();
    }

    pub fn save_connection(
        &mut self,
        instance_url: &str,
        mcp_url: &str,
        entered_key: &str,
        entered_access_token: &str,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        self.status = PenpotConnectionStatus::Checking;
        self.last_error = None;
        cx.notify();

        let result = self.save_connection_inner(
            instance_url,
            mcp_url,
            entered_key,
            entered_access_token,
            cx,
        );
        if let Err(error) = result {
            let message =
                redact_penpot_error(&error.to_string(), [entered_key, entered_access_token]);
            self.status = PenpotConnectionStatus::Error(message.clone());
            self.last_error = Some(message.clone());
            cx.emit(PenpotEvent::Changed);
            cx.notify();
            return Err(anyhow!(message));
        }
        Ok(())
    }

    fn save_connection_inner(
        &mut self,
        instance_url: &str,
        mcp_url: &str,
        entered_key: &str,
        entered_access_token: &str,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        let (config, embedded_key) = PenpotConfig::normalized(instance_url, mcp_url)?;
        let entered_key = entered_key.trim().to_string();
        let key = if entered_key.is_empty() {
            embedded_key
        } else {
            Some(entered_key)
        };
        let entered_access_token = entered_access_token.trim();
        let access_token = if entered_access_token.is_empty() {
            self.connection
                .as_ref()
                .filter(|connection| connection.instance_url == config.instance_url)
                .and_then(|connection| load_access_token(connection.id))
        } else {
            Some(entered_access_token.to_string())
        };
        let Some(access_token) = access_token else {
            return Err(anyhow!(
                "Enter a Design access token so Choro can create designs"
            ));
        };
        // Resolve the account before changing active state. This prevents a new
        // token on the same Penpot host from inheriting another account's files.
        let resolved = resolve_penpot_workspace(&config.instance_url, &access_token)
            .map_err(anyhow::Error::msg)?;
        let profile_id = json_uuid(&resolved.profile, "id");
        let profile_email = resolved
            .profile
            .get("email")
            .and_then(|value| value.as_str())
            .map(str::to_string);
        let store = LocalStore::open_default()?;
        let known_connection = profile_id
            .map(|profile_id| store.penpot_connection_for_profile(&config.instance_url, profile_id))
            .transpose()?
            .flatten();
        let same_account = self.connection.as_ref().is_some_and(|existing| {
            existing.instance_url == config.instance_url
                && match (existing.profile_id, profile_id) {
                    (Some(left), Some(right)) => left == right,
                    (None, _) => self.designs.values().all(Vec::is_empty),
                    _ => false,
                }
        });
        let now = ide_core::agents::unix_now();
        let connection_id = known_connection
            .as_ref()
            .map(|value| value.id)
            .or_else(|| {
                self.connection
                    .as_ref()
                    .filter(|_| same_account)
                    .map(|value| value.id)
            })
            .unwrap_or_else(Uuid::new_v4);
        if let Some(key) = key.as_deref() {
            save_mcp_key(connection_id, key)?;
        } else if !config.is_local() && load_mcp_key(connection_id).is_none() {
            return Err(anyhow!("Enter the Design MCP key"));
        }
        save_access_token(connection_id, &access_token)?;
        let connection = StoredPenpotConnection {
            id: connection_id,
            instance_url: config.instance_url.clone(),
            mcp_url: config.mcp_url.clone(),
            profile_id,
            profile_email,
            default_team_id: Some(resolved.team_id),
            default_project_id: Some(resolved.project_id),
            is_active: true,
            verified_at: Some(now),
            created_at: known_connection
                .as_ref()
                .map(|value| value.created_at)
                .or_else(|| {
                    self.connection
                        .as_ref()
                        .filter(|_| same_account)
                        .map(|value| value.created_at)
                })
                .unwrap_or(now),
            updated_at: now,
        };
        store.save_active_penpot_connection(&connection)?;
        self.config = config;
        self.connection = Some(connection);
        self.designs = store
            .load_all_penpot_designs()?
            .into_iter()
            .filter(|design| design.connection_id == connection_id)
            .fold(HashMap::new(), |mut values, design| {
                values.entry(design.project_id).or_default().push(design);
                values
            });
        self.conversations = load_conversations(&store, &self.designs);
        self.selected_designs.clear();
        for project in self.designs.keys().copied() {
            if let Some(selected) = store
                .project_penpot_binding(project, connection_id)?
                .filter(|binding| binding.connection_id == connection_id)
                .and_then(|binding| binding.selected_design_id)
            {
                self.selected_designs.insert(project, selected);
            }
        }
        self.has_key = load_mcp_key(connection_id).is_some() || self.config.is_local();
        self.has_access_token = load_access_token(connection_id).is_some();
        self.status = PenpotConnectionStatus::NotChecked;
        self.last_error = None;
        cx.emit(PenpotEvent::Changed);
        cx.notify();
        Ok(())
    }

    pub fn test_connection(&mut self, cx: &mut Context<Self>) {
        if matches!(self.status, PenpotConnectionStatus::Checking) {
            return;
        }
        let Some(connection) = self.connection.clone() else {
            self.status =
                PenpotConnectionStatus::Error("Save the Design connection first.".to_string());
            cx.notify();
            return;
        };
        let key = load_mcp_key(connection.id);
        let url = match self.config.full_mcp_url(key.as_deref()) {
            Ok(url) => url,
            Err(error) => {
                self.status = PenpotConnectionStatus::Error(error.to_string());
                cx.notify();
                return;
            }
        };
        let instance_url = self.config.instance_url.clone();
        let Some(access_token) = load_access_token(connection.id) else {
            self.status =
                PenpotConnectionStatus::Error("Design access token is missing.".to_string());
            cx.notify();
            return;
        };
        self.status = PenpotConnectionStatus::Checking;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let resolved = resolve_penpot_workspace(&instance_url, &access_token)?;
                    probe_mcp_server(url)?;
                    Ok(resolved)
                })
                .await;
            this.update(cx, |this, cx| {
                this.status = match result {
                    Ok(resolved) => {
                        let profile_id = json_uuid(&resolved.profile, "id");
                        let profile_email = resolved
                            .profile
                            .get("email")
                            .and_then(|value| value.as_str());
                        let persisted = LocalStore::open_default().and_then(|store| {
                            store.update_penpot_connection_profile(
                                connection.id,
                                profile_id,
                                profile_email,
                                resolved.team_id,
                                resolved.project_id,
                            )
                        });
                        if let Err(error) = persisted {
                            PenpotConnectionStatus::Error(redact_penpot_error(
                                &error.to_string(),
                                std::iter::empty::<&str>(),
                            ))
                        } else {
                            if let Some(active) = this
                                .connection
                                .as_mut()
                                .filter(|active| active.id == connection.id)
                            {
                                active.profile_id = profile_id;
                                active.profile_email = profile_email.map(str::to_string);
                                active.default_team_id = Some(resolved.team_id);
                                active.default_project_id = Some(resolved.project_id);
                                active.verified_at = Some(ide_core::agents::unix_now());
                                active.updated_at = ide_core::agents::unix_now();
                            }
                            this.last_error = None;
                            PenpotConnectionStatus::Reachable
                        }
                    }
                    Err(error) => PenpotConnectionStatus::Error(error),
                };
                cx.emit(PenpotEvent::Changed);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn create_design(
        &mut self,
        project: ProjectId,
        name: String,
        source: Option<PenpotDesignSource>,
        initial_prompt: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if self.creating_design || self.assistant_busy(project) {
            return;
        }
        let Some(connection) = self.connection.clone() else {
            self.last_error = Some("Save the Design connection first.".to_string());
            cx.notify();
            return;
        };
        let Some(access_token) = load_access_token(connection.id) else {
            self.last_error =
                Some("Add a Design access token in Design settings first.".to_string());
            cx.notify();
            return;
        };
        let instance_url = self.config.instance_url.clone();
        let stored_team_id = connection.default_team_id;
        let stored_project_id = connection.default_project_id;
        self.creating_design = true;
        self.last_error = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    create_penpot_design(
                        &instance_url,
                        &access_token,
                        name.as_str(),
                        stored_team_id,
                        stored_project_id,
                    )
                })
                .await;
            this.update(cx, |this, cx| {
                this.creating_design = false;
                match result {
                    Ok(remote) => {
                        let now = ide_core::agents::unix_now();
                        if let Some(resolved) = remote.resolved_workspace.as_ref() {
                            let profile_id = json_uuid(&resolved.profile, "id");
                            let profile_email = resolved
                                .profile
                                .get("email")
                                .and_then(|value| value.as_str());
                            if let Err(error) = LocalStore::open_default().and_then(|store| {
                                store.update_penpot_connection_profile(
                                    connection.id,
                                    profile_id,
                                    profile_email,
                                    resolved.team_id,
                                    resolved.project_id,
                                )
                            }) {
                                this.last_error = Some(redact_penpot_error(
                                    &error.to_string(),
                                    std::iter::empty::<&str>(),
                                ));
                                cx.emit(PenpotEvent::Changed);
                                cx.notify();
                                return;
                            }
                            if let Some(active) = this
                                .connection
                                .as_mut()
                                .filter(|active| active.id == connection.id)
                            {
                                active.profile_id = profile_id;
                                active.profile_email = profile_email.map(str::to_string);
                                active.default_team_id = Some(resolved.team_id);
                                active.default_project_id = Some(resolved.project_id);
                                active.verified_at = Some(now);
                                active.updated_at = now;
                            }
                        }
                        let (source_doc, source_task) = match source.clone() {
                            Some(PenpotDesignSource::Document(path)) => (Some(path), None),
                            Some(PenpotDesignSource::Task(reference)) => (None, Some(reference)),
                            None => (None, None),
                        };
                        let design = PenpotDesign {
                            id: Uuid::new_v4(),
                            project_id: project,
                            connection_id: connection.id,
                            penpot_file_id: remote.id,
                            penpot_project_id: remote.penpot_project_id,
                            penpot_team_id: remote.team_id,
                            name: remote.name,
                            page_id: remote.page_id,
                            source_doc,
                            source_task,
                            last_synced_at: Some(now),
                            archived_at: None,
                            created_at: now,
                            updated_at: now,
                        };
                        let design_id = design.id;
                        let binding = StoredProjectPenpotBinding {
                            project_id: project,
                            connection_id: connection.id,
                            penpot_team_id: design.penpot_team_id,
                            penpot_project_id: design.penpot_project_id,
                            selected_design_id: Some(design_id),
                            created_at: now,
                            updated_at: now,
                        };
                        let persisted = LocalStore::open_default().and_then(|store| {
                            store.upsert_project_penpot_binding(&binding)?;
                            store.upsert_penpot_design(&design)?;
                            store.select_penpot_design(project, design_id)?;
                            let conversation =
                                store.ensure_current_penpot_conversation(design_id)?;
                            Ok(conversation)
                        });
                        match persisted {
                            Ok(conversation) => {
                                this.designs.entry(project).or_default().push(design);
                                this.selected_designs.insert(project, design_id);
                                this.conversations.insert(design_id, vec![conversation]);
                                cx.emit(PenpotEvent::DesignCreated {
                                    project,
                                    design_id,
                                    initial_prompt: initial_prompt.clone(),
                                });
                            }
                            Err(error) => {
                                this.last_error = Some(error.to_string());
                                cx.emit(PenpotEvent::Changed);
                            }
                        }
                    }
                    Err(error) => {
                        this.last_error = Some(error);
                        cx.emit(PenpotEvent::Changed);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn delete_design(&mut self, project: ProjectId, design_id: Uuid, cx: &mut Context<Self>) {
        let Some(design) = self.design(project, design_id) else {
            return;
        };
        let Some(connection) = self
            .connection
            .clone()
            .filter(|connection| connection.id == design.connection_id)
        else {
            self.last_error =
                Some("Reconnect this design account before deleting the design.".to_string());
            cx.emit(PenpotEvent::Changed);
            cx.notify();
            return;
        };
        let Some(access_token) = load_access_token(connection.id) else {
            self.last_error = Some("Reconnect Design before deleting this design.".to_string());
            cx.emit(PenpotEvent::Changed);
            cx.notify();
            return;
        };
        let instance_url = self.config.instance_url.clone();
        self.last_error = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    delete_penpot_design(&instance_url, &access_token, design.penpot_file_id)?;
                    LocalStore::open_default()
                        .and_then(|store| store.delete_penpot_design(project, design_id))
                        .map_err(|error| error.to_string())
                })
                .await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        let mut remove_project = false;
                        if let Some(designs) = this.designs.get_mut(&project) {
                            designs.retain(|design| design.id != design_id);
                            remove_project = designs.is_empty();
                        }
                        if remove_project {
                            this.designs.remove(&project);
                        }
                        if this.selected_designs.get(&project) == Some(&design_id) {
                            this.selected_designs.remove(&project);
                        }
                        this.conversations.remove(&design_id);
                        if let Some(path) = this.thumbnail_paths.remove(&design_id) {
                            let _ = fs::remove_file(path);
                        }
                        this.last_error = None;
                    }
                    Err(error) => {
                        this.last_error =
                            Some(redact_penpot_error(&error, std::iter::empty::<&str>()));
                    }
                }
                cx.emit(PenpotEvent::Changed);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

fn load_conversations(
    store: &LocalStore,
    designs: &HashMap<ProjectId, Vec<PenpotDesign>>,
) -> HashMap<Uuid, Vec<StoredPenpotDesignConversation>> {
    designs
        .values()
        .flatten()
        .filter_map(|design| {
            let current = store.ensure_current_penpot_conversation(design.id).ok()?;
            let mut conversations = store.load_penpot_conversations(design.id).ok()?;
            if !conversations
                .iter()
                .any(|conversation| conversation.id == current.id)
            {
                conversations.push(current);
            }
            Some((design.id, conversations))
        })
        .collect()
}

/// Read the configured remote URL for agent startup. The key is retrieved only
/// at the last possible moment and is never persisted in agent records.
pub fn configured_mcp_url() -> Option<String> {
    let connection = LocalStore::open_default()
        .and_then(|store| store.active_penpot_connection())
        .ok()
        .flatten()?;
    let config = PenpotConfig {
        instance_url: connection.instance_url,
        mcp_url: connection.mcp_url,
        designs: HashMap::new(),
        selected_designs: HashMap::new(),
    };
    config
        .full_mcp_url(load_mcp_key(connection.id).as_deref())
        .ok()
}

fn probe_mcp_server(url: String) -> std::result::Result<(), String> {
    let known_key = Url::parse(&url).ok().and_then(|url| {
        url.query_pairs()
            .find(|(name, _)| name == "userToken")
            .map(|(_, value)| value.into_owned())
    });
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| "Could not prepare the MCP connection.".to_string())?;
    let response = client
        .post(url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header(
            reqwest::header::ACCEPT,
            "application/json, text/event-stream",
        )
        .body(
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-03-26",
                    "capabilities": {},
                    "clientInfo": {
                        "name": "Choro",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                }
            })
            .to_string(),
        )
        .send()
        .map_err(|error| {
            let redacted = ide_core::redaction::redact_sensitive_text_with(
                &error.to_string(),
                known_key.as_deref(),
            );
            format!("Could not reach the Design MCP server: {redacted}")
        })?;
    if response.status().is_success() {
        Ok(())
    } else if response.status() == reqwest::StatusCode::UNAUTHORIZED
        || response.status() == reqwest::StatusCode::FORBIDDEN
    {
        Err("The Design service rejected the MCP key. Generate or copy the key again.".to_string())
    } else {
        Err(format!(
            "Design MCP returned HTTP {}.",
            response.status().as_u16()
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ResolvedPenpotWorkspace {
    profile: serde_json::Value,
    team_id: Uuid,
    project_id: Uuid,
}

fn resolve_penpot_workspace(
    instance_url: &str,
    access_token: &str,
) -> std::result::Result<ResolvedPenpotWorkspace, String> {
    resolve_penpot_workspace_with(access_token, |method, body| {
        penpot_api_call(instance_url, access_token, method, body)
    })
}

fn resolve_penpot_workspace_with<F>(
    access_token: &str,
    mut call: F,
) -> std::result::Result<ResolvedPenpotWorkspace, String>
where
    F: FnMut(&str, serde_json::Value) -> std::result::Result<serde_json::Value, String>,
{
    let profile = call_penpot_resolver_api(&mut call, access_token, "get-profile", json!({}))?;
    let default_project_id = json_uuid(&profile, "defaultProjectId");
    let default_team_id = json_uuid(&profile, "defaultTeamId");

    if let (Some(project_id), Some(team_id)) = (default_project_id, default_team_id) {
        return Ok(ResolvedPenpotWorkspace {
            profile,
            team_id,
            project_id,
        });
    }

    if let Some(project_id) = default_project_id {
        let project = call_penpot_resolver_api(
            &mut call,
            access_token,
            "get-project",
            json!({ "id": project_id }),
        )?;
        if let Some(team_id) = json_uuid(&project, "teamId") {
            return Ok(ResolvedPenpotWorkspace {
                profile,
                team_id,
                project_id,
            });
        }
    }

    let all_projects =
        call_penpot_resolver_api(&mut call, access_token, "get-all-projects", json!({}))?;
    if let Some((team_id, project_id)) =
        select_penpot_project(response_items(all_projects, "projects"))
    {
        return Ok(ResolvedPenpotWorkspace {
            profile,
            team_id,
            project_id,
        });
    }

    let teams = call_penpot_resolver_api(&mut call, access_token, "get-teams", json!({}))?;
    let Some(team_id) = select_penpot_team(response_items(teams, "teams")) else {
        return Err(NO_WORKSPACE_ERROR.to_string());
    };
    let projects = call_penpot_resolver_api(
        &mut call,
        access_token,
        "get-projects",
        json!({ "teamId": team_id }),
    )?;
    let Some(project_id) = select_project_for_team(response_items(projects, "projects")) else {
        return Err(NO_WORKSPACE_ERROR.to_string());
    };

    Ok(ResolvedPenpotWorkspace {
        profile,
        team_id,
        project_id,
    })
}

fn call_penpot_resolver_api<F>(
    call: &mut F,
    access_token: &str,
    method: &str,
    body: serde_json::Value,
) -> std::result::Result<serde_json::Value, String>
where
    F: FnMut(&str, serde_json::Value) -> std::result::Result<serde_json::Value, String>,
{
    call(method, body).map_err(|error| redact_penpot_error(&error, [access_token]))
}

fn response_items(value: serde_json::Value, collection_key: &str) -> Vec<serde_json::Value> {
    match value {
        serde_json::Value::Array(items) => items,
        serde_json::Value::Object(mut object) => object
            .remove(collection_key)
            .or_else(|| object.remove("data"))
            .and_then(|value| value.as_array().cloned())
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn select_penpot_project(projects: Vec<serde_json::Value>) -> Option<(Uuid, Uuid)> {
    select_by_priority(projects, |project| {
        let project_id = json_uuid(project, "id")?;
        let team_id = json_uuid(project, "teamId")?;
        let priority = match (
            json_bool(project, "isDefaultTeam"),
            json_bool(project, "isDefault"),
        ) {
            (true, true) => 2,
            (_, true) => 1,
            _ => 0,
        };
        Some((priority, (team_id, project_id)))
    })
}

fn select_penpot_team(teams: Vec<serde_json::Value>) -> Option<Uuid> {
    select_by_priority(teams, |team| {
        let team_id = json_uuid(team, "id")?;
        Some((u8::from(json_bool(team, "isDefault")), team_id))
    })
}

fn select_project_for_team(projects: Vec<serde_json::Value>) -> Option<Uuid> {
    select_by_priority(projects, |project| {
        let project_id = json_uuid(project, "id")?;
        Some((u8::from(json_bool(project, "isDefault")), project_id))
    })
}

fn select_by_priority<T, U>(
    values: Vec<T>,
    mut candidate: impl FnMut(&T) -> Option<(u8, U)>,
) -> Option<U> {
    let mut selected = None;
    let mut selected_priority = 0;
    for value in &values {
        let Some((priority, candidate)) = candidate(value) else {
            continue;
        };
        if selected.is_none() || priority > selected_priority {
            selected = Some(candidate);
            selected_priority = priority;
        }
    }
    selected
}

fn create_penpot_design(
    instance_url: &str,
    access_token: &str,
    name: &str,
    stored_team_id: Option<Uuid>,
    stored_project_id: Option<Uuid>,
) -> std::result::Result<RemotePenpotDesign, String> {
    create_penpot_design_with(
        access_token,
        name,
        stored_team_id,
        stored_project_id,
        |method, body| penpot_api_call(instance_url, access_token, method, body),
    )
}

fn delete_penpot_design(
    instance_url: &str,
    access_token: &str,
    penpot_file_id: Uuid,
) -> std::result::Result<(), String> {
    delete_penpot_design_with(penpot_file_id, |method, body| {
        penpot_api_call(instance_url, access_token, method, body)
    })
}

fn delete_penpot_design_with<F>(
    penpot_file_id: Uuid,
    mut call: F,
) -> std::result::Result<(), String>
where
    F: FnMut(&str, serde_json::Value) -> std::result::Result<serde_json::Value, String>,
{
    match call("delete-file", json!({ "id": penpot_file_id })) {
        Ok(_) => Ok(()),
        Err(error) if is_missing_penpot_file_error(&error) => Ok(()),
        Err(error) => Err(error),
    }
}

fn is_missing_penpot_file_error(error: &str) -> bool {
    error.starts_with("Design service returned HTTP 404:")
}

fn create_penpot_design_with<F>(
    access_token: &str,
    name: &str,
    stored_team_id: Option<Uuid>,
    stored_project_id: Option<Uuid>,
    mut call: F,
) -> std::result::Result<RemotePenpotDesign, String>
where
    F: FnMut(&str, serde_json::Value) -> std::result::Result<serde_json::Value, String>,
{
    let (team_id, penpot_project_id, resolved_workspace) = match (stored_team_id, stored_project_id)
    {
        (Some(team_id), Some(project_id)) => (team_id, project_id, None),
        _ => {
            let resolved = resolve_penpot_workspace_with(access_token, &mut call)?;
            (resolved.team_id, resolved.project_id, Some(resolved))
        }
    };
    let file = call_penpot_resolver_api(
        &mut call,
        access_token,
        "create-file",
        json!({
            "name": name,
            "projectId": penpot_project_id,
        }),
    )?;
    let id = json_uuid(&file, "id").ok_or_else(|| {
        "The Design service created the design but did not return its file ID.".to_string()
    })?;
    let page_id = file
        .get("pages")
        .and_then(|pages| pages.as_array())
        .and_then(|pages| pages.first())
        .and_then(|page| {
            page.as_str()
                .or_else(|| page.get("id").and_then(|id| id.as_str()))
        })
        .and_then(|page| Uuid::parse_str(page).ok());
    Ok(RemotePenpotDesign {
        id,
        penpot_project_id,
        team_id,
        name: file
            .get("name")
            .and_then(|name| name.as_str())
            .unwrap_or(name)
            .to_string(),
        page_id,
        resolved_workspace,
    })
}

fn fetch_penpot_design_thumbnails(
    instance_url: &str,
    access_token: &str,
    designs: &[PenpotDesign],
) -> std::result::Result<HashMap<Uuid, PathBuf>, String> {
    let mut project_thumbnail_ids: HashMap<Uuid, HashMap<Uuid, Uuid>> = HashMap::new();
    for penpot_project_id in designs
        .iter()
        .map(|design| design.penpot_project_id)
        .collect::<HashSet<_>>()
    {
        let files = penpot_api_call(
            instance_url,
            access_token,
            "get-project-files",
            json!({ "projectId": penpot_project_id }),
        )?;
        let thumbnails = response_items(files, "files")
            .into_iter()
            .filter_map(|file| Some((json_uuid(&file, "id")?, json_uuid(&file, "thumbnailId")?)))
            .collect();
        project_thumbnail_ids.insert(penpot_project_id, thumbnails);
    }

    let mut thumbnails = HashMap::new();
    for design in designs {
        let file_thumbnail = project_thumbnail_ids
            .get(&design.penpot_project_id)
            .and_then(|files| files.get(&design.penpot_file_id))
            .copied();
        let media_id = match file_thumbnail {
            Some(media_id) => Some(media_id),
            None => penpot_api_call(
                instance_url,
                access_token,
                "get-file-object-thumbnails",
                json!({
                    "fileId": design.penpot_file_id,
                    "tag": "frame",
                }),
            )
            .ok()
            .and_then(first_penpot_thumbnail_id),
        };
        let Some(media_id) = media_id else {
            continue;
        };
        if let Ok(path) = download_penpot_thumbnail(instance_url, access_token, design.id, media_id)
        {
            thumbnails.insert(design.id, path);
        }
    }
    Ok(thumbnails)
}

fn first_penpot_thumbnail_id(value: serde_json::Value) -> Option<Uuid> {
    let object = value.as_object()?;
    let mut entries = object.iter().collect::<Vec<_>>();
    entries.sort_by(|(left, _), (right, _)| left.cmp(right));
    entries
        .into_iter()
        .find_map(|(_, value)| value.as_str().and_then(|value| Uuid::parse_str(value).ok()))
}

fn download_penpot_thumbnail(
    instance_url: &str,
    access_token: &str,
    design_id: Uuid,
    media_id: Uuid,
) -> std::result::Result<PathBuf, String> {
    let endpoint = format!(
        "{}/assets/by-id/{media_id}",
        instance_url.trim_end_matches('/')
    );
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|_| "Could not prepare the design thumbnail request.".to_string())?;
    let response = client
        .get(endpoint)
        .header(
            reqwest::header::AUTHORIZATION,
            format!("Token {access_token}"),
        )
        .send()
        .map_err(|error| {
            let redacted = ide_core::redaction::redact_sensitive_text_with(
                &error.to_string(),
                Some(access_token),
            );
            format!("Could not load the design thumbnail: {redacted}")
        })?;
    if !response.status().is_success() {
        return Err(format!(
            "Design thumbnail returned HTTP {}.",
            response.status().as_u16()
        ));
    }
    let bytes = response
        .bytes()
        .map_err(|_| "The Design service returned an unreadable thumbnail.".to_string())?;
    let format = image::guess_format(&bytes)
        .map_err(|_| "The Design service returned an unsupported thumbnail image.".to_string())?;
    let extension = match format {
        image::ImageFormat::Png => "png",
        image::ImageFormat::Jpeg => "jpg",
        image::ImageFormat::WebP => "webp",
        image::ImageFormat::Gif => "gif",
        _ => {
            return Err("The Design service returned an unsupported thumbnail image.".to_string());
        }
    };
    let cache_dir = ide_core::AppConfig::config_root()
        .join("cache")
        .join("penpot-thumbnails");
    fs::create_dir_all(&cache_dir)
        .map_err(|_| "Could not prepare the design thumbnail cache.".to_string())?;
    let path = cache_dir.join(format!("{design_id}-{media_id}.{extension}"));
    fs::write(&path, bytes).map_err(|_| "Could not cache the design thumbnail.".to_string())?;
    Ok(path)
}

struct RemotePenpotDesign {
    id: Uuid,
    penpot_project_id: Uuid,
    team_id: Uuid,
    name: String,
    page_id: Option<Uuid>,
    resolved_workspace: Option<ResolvedPenpotWorkspace>,
}

fn penpot_api_call(
    instance_url: &str,
    access_token: &str,
    method: &str,
    body: serde_json::Value,
) -> std::result::Result<serde_json::Value, String> {
    let endpoint = format!(
        "{}/api/main/methods/{method}",
        instance_url.trim_end_matches('/')
    );
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|_| "Could not prepare the Design service connection.".to_string())?;
    let response = client
        .post(endpoint)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header(reqwest::header::ACCEPT, "application/json")
        .header(
            reqwest::header::AUTHORIZATION,
            format!("Token {access_token}"),
        )
        .body(body.to_string())
        .send()
        .map_err(|error| {
            let redacted = ide_core::redaction::redact_sensitive_text_with(
                &error.to_string(),
                Some(access_token),
            );
            format!("Could not reach the Design service: {redacted}")
        })?;
    let status = response.status();
    let response_body = response.text().unwrap_or_default();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(
            "The Design service rejected the access token. Generate a new token in Your account → Access tokens."
                .to_string(),
        );
    }
    if !status.is_success() {
        let safe_body =
            ide_core::redaction::redact_sensitive_text_with(&response_body, Some(access_token));
        return Err(format!(
            "Design service returned HTTP {}: {}",
            status.as_u16(),
            safe_body.trim()
        ));
    }
    let value = decode_penpot_response_body(&response_body)?;
    Ok(decode_penpot_transit(value))
}

fn decode_penpot_response_body(
    response_body: &str,
) -> std::result::Result<serde_json::Value, String> {
    if response_body.trim().is_empty() {
        return Ok(serde_json::Value::Null);
    }
    serde_json::from_str(response_body)
        .map_err(|_| "The Design service returned an unreadable response.".to_string())
}

fn decode_penpot_transit(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Array(values)
            if values.first().and_then(serde_json::Value::as_str) == Some("^ ") =>
        {
            let mut object = serde_json::Map::new();
            let mut values = values.into_iter().skip(1);
            while let (Some(key), Some(value)) = (values.next(), values.next()) {
                let key = decode_penpot_transit(key);
                let Some(key) = key.as_str() else {
                    continue;
                };
                object.insert(key.to_string(), decode_penpot_transit(value));
            }
            serde_json::Value::Object(object)
        }
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.into_iter().map(decode_penpot_transit).collect())
        }
        serde_json::Value::Object(values) => serde_json::Value::Object(
            values
                .into_iter()
                .map(|(key, value)| (camel_case_penpot_key(&key), decode_penpot_transit(value)))
                .collect(),
        ),
        serde_json::Value::String(value) if value.starts_with("~:") => {
            serde_json::Value::String(camel_case_penpot_key(&value[2..]))
        }
        serde_json::Value::String(value) if value.starts_with("~u") || value.starts_with("~m") => {
            serde_json::Value::String(value[2..].to_string())
        }
        value => value,
    }
}

fn camel_case_penpot_key(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut uppercase = false;
    for character in value.chars() {
        if character == '-' {
            uppercase = true;
        } else if uppercase {
            result.extend(character.to_uppercase());
            uppercase = false;
        } else {
            result.push(character);
        }
    }
    result
}

#[derive(Clone, Debug)]
struct InstallationIdentity {
    id: Uuid,
    secret: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManagedPenpotBootstrap {
    instance_url: String,
    mcp_url: String,
    profile_id: Uuid,
    profile_email: String,
    team_id: Uuid,
    project_id: Uuid,
    api_token: String,
    mcp_key: String,
    session_token: String,
    #[serde(default)]
    starter_design: Option<ManagedStarterDesign>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManagedStarterDesign {
    file_id: Uuid,
    project_id: Uuid,
    team_id: Uuid,
    name: String,
    page_id: Option<Uuid>,
}

fn bootstrap_managed_penpot() -> std::result::Result<ManagedPenpotBootstrap, String> {
    let installation = load_or_create_installation_identity().map_err(|error| error.to_string())?;
    let endpoint = managed_provisioning_url();
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(45))
        .build()
        .map_err(|_| "Could not prepare the Design service connection.".to_string())?;
    let response = client
        .post(endpoint)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header(reqwest::header::ACCEPT, "application/json")
        .body(
            json!({
                "installationId": installation.id,
                "installationSecret": installation.secret,
                "includeStarterDesign": managed_starter_project_id().is_some(),
            })
            .to_string(),
        )
        .send()
        .map_err(|_| "Could not reach the Design service. Try again shortly.".to_string())?;
    if !response.status().is_success() {
        return Err(
            if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                "Design setup is temporarily busy. Try again shortly.".to_string()
            } else {
                "Could not prepare your Design workspace. Try again shortly.".to_string()
            },
        );
    }
    let body = response
        .text()
        .map_err(|_| "The Design service returned an unreadable response.".to_string())?;
    serde_json::from_str(&body)
        .map_err(|_| "The Design service returned an unreadable response.".to_string())
}

fn managed_starter_project_id() -> Option<ProjectId> {
    if !crate::onboarding::enabled() {
        return None;
    }
    let store = LocalStore::open_default().ok()?;
    let config = store
        .load_workspace_config(ide_core::AppConfig::load())
        .ok()?;
    let playground_root = crate::onboarding::playground_root();
    config
        .projects
        .into_iter()
        .find(|project| project.path == playground_root)
        .map(|project| project.id)
}

fn persist_managed_starter_design(
    store: &LocalStore,
    project: ProjectId,
    connection_id: Uuid,
    starter: &ManagedStarterDesign,
    now: u64,
) -> Result<()> {
    if store
        .load_penpot_designs(project)?
        .into_iter()
        .any(|design| {
            design.connection_id == connection_id
                && design.penpot_file_id == starter.file_id
                && design.archived_at.is_none()
        })
    {
        return Ok(());
    }

    let design = starter_design_record(project, connection_id, starter, now);
    let existing_binding = store.project_penpot_binding(project, connection_id)?;
    let binding = StoredProjectPenpotBinding {
        project_id: project,
        connection_id,
        penpot_team_id: starter.team_id,
        penpot_project_id: starter.project_id,
        selected_design_id: existing_binding
            .as_ref()
            .and_then(|binding| binding.selected_design_id),
        created_at: existing_binding
            .as_ref()
            .map(|binding| binding.created_at)
            .unwrap_or(now),
        updated_at: now,
    };
    store.upsert_project_penpot_binding(&binding)?;
    store.upsert_penpot_design(&design)?;
    store.ensure_current_penpot_conversation(design.id)?;
    Ok(())
}

fn starter_design_record(
    project: ProjectId,
    connection_id: Uuid,
    starter: &ManagedStarterDesign,
    now: u64,
) -> PenpotDesign {
    PenpotDesign {
        id: Uuid::new_v4(),
        project_id: project,
        connection_id,
        penpot_file_id: starter.file_id,
        penpot_project_id: starter.project_id,
        penpot_team_id: starter.team_id,
        name: starter.name.clone(),
        page_id: starter.page_id,
        source_doc: None,
        source_task: None,
        last_synced_at: Some(now),
        archived_at: None,
        created_at: now,
        updated_at: now,
    }
}

fn managed_provisioning_url() -> String {
    std::env::var("CHORO_PENPOT_PROVISIONING_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_PROVISIONING_URL.to_string())
}

fn instance_matches_managed_service(instance_url: &str) -> bool {
    same_http_origin(instance_url, &managed_provisioning_url())
}

fn design_provider_for_connection(connection: Option<&StoredPenpotConnection>) -> DesignProvider {
    connection
        .filter(|connection| !instance_matches_managed_service(&connection.instance_url))
        .map(|_| DesignProvider::PenpotCloud)
        .unwrap_or(DesignProvider::Choro)
}

fn should_auto_provision_connection(connection: Option<&StoredPenpotConnection>) -> bool {
    connection.is_none()
        || connection
            .is_some_and(|connection| instance_matches_managed_service(&connection.instance_url))
}

fn same_http_origin(left: &str, right: &str) -> bool {
    let Ok(left) = Url::parse(left) else {
        return false;
    };
    let Ok(right) = Url::parse(right) else {
        return false;
    };
    left.scheme() == right.scheme()
        && left.host_str() == right.host_str()
        && left.port_or_known_default() == right.port_or_known_default()
}

fn json_uuid(value: &serde_json::Value, key: &str) -> Option<Uuid> {
    value
        .get(key)
        .and_then(|value| value.as_str())
        .and_then(|value| Uuid::parse_str(value).ok())
}

fn json_bool(value: &serde_json::Value, key: &str) -> bool {
    value.get(key).and_then(|value| value.as_bool()) == Some(true)
}

fn redact_penpot_error<'a>(
    error: &str,
    known_secrets: impl IntoIterator<Item = &'a str>,
) -> String {
    ide_core::redaction::redact_sensitive_text_with(error, known_secrets)
        .replace("Penpot", "Design service")
        .replace("penpot", "design service")
}

fn default_instance_url() -> String {
    DEFAULT_INSTANCE_URL.to_string()
}

fn default_mcp_url() -> String {
    DEFAULT_MCP_URL.to_string()
}

fn normalize_http_url(value: &str, label: &str) -> Result<String> {
    let mut url =
        Url::parse(value.trim()).with_context(|| format!("{label} is not a valid URL"))?;
    anyhow::ensure!(
        matches!(url.scheme(), "http" | "https"),
        "{label} must use http or https"
    );
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.to_string().trim_end_matches('/').to_string())
}

#[cfg(target_os = "macos")]
fn save_mcp_key(connection_id: Uuid, key: &str) -> Result<()> {
    save_keychain_secret(&format!("mcp:{connection_id}"), key)
}

#[cfg(target_os = "macos")]
fn save_access_token(connection_id: Uuid, token: &str) -> Result<()> {
    save_keychain_secret(&format!("api:{connection_id}"), token)
}

#[cfg(target_os = "macos")]
fn save_session_token(connection_id: Uuid, token: &str) -> Result<()> {
    save_keychain_secret(&format!("session:{connection_id}"), token)
}

#[cfg(target_os = "macos")]
fn save_keychain_secret(account: &str, value: &str) -> Result<()> {
    security_framework::passwords::set_generic_password(
        &penpot_keychain_service(),
        account,
        value.as_bytes(),
    )
    .map_err(|error| anyhow!("Could not save the Design credential in Keychain: {error}"))
}

#[cfg(target_os = "macos")]
fn load_mcp_key(connection_id: Uuid) -> Option<String> {
    load_keychain_secret(&format!("mcp:{connection_id}"))
}

#[cfg(target_os = "macos")]
fn load_access_token(connection_id: Uuid) -> Option<String> {
    load_keychain_secret(&format!("api:{connection_id}"))
}

#[cfg(target_os = "macos")]
fn load_session_token(connection_id: Uuid) -> Option<String> {
    load_keychain_secret(&format!("session:{connection_id}"))
}

#[cfg(target_os = "macos")]
fn load_keychain_secret(account: &str) -> Option<String> {
    let bytes =
        security_framework::passwords::get_generic_password(&penpot_keychain_service(), account)
            .ok()?;
    String::from_utf8(bytes).ok()
}

#[cfg(target_os = "macos")]
fn penpot_keychain_service() -> String {
    std::env::var("CHORO_PENPOT_KEYCHAIN_SERVICE")
        .ok()
        .filter(|service| !service.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_PENPOT_KEYCHAIN_SERVICE.to_string())
}

#[cfg(target_os = "macos")]
fn load_or_create_installation_identity() -> Result<InstallationIdentity> {
    let keychain_service = installation_keychain_service();
    let existing_id = security_framework::passwords::get_generic_password(
        &keychain_service,
        INSTALLATION_ID_ACCOUNT,
    )
    .ok()
    .and_then(|value| String::from_utf8(value).ok())
    .and_then(|value| Uuid::parse_str(&value).ok());
    let existing_secret = security_framework::passwords::get_generic_password(
        &keychain_service,
        INSTALLATION_SECRET_ACCOUNT,
    )
    .ok()
    .and_then(|value| String::from_utf8(value).ok())
    .filter(|value| value.len() >= 32);
    if let (Some(id), Some(secret)) = (existing_id, existing_secret) {
        return Ok(InstallationIdentity { id, secret });
    }
    let identity = InstallationIdentity {
        id: Uuid::new_v4(),
        secret: format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple()),
    };
    security_framework::passwords::set_generic_password(
        &keychain_service,
        INSTALLATION_ID_ACCOUNT,
        identity.id.to_string().as_bytes(),
    )
    .map_err(|error| anyhow!("Could not save the Choro installation identity: {error}"))?;
    security_framework::passwords::set_generic_password(
        &keychain_service,
        INSTALLATION_SECRET_ACCOUNT,
        identity.secret.as_bytes(),
    )
    .map_err(|error| anyhow!("Could not save the Choro installation credential: {error}"))?;
    Ok(identity)
}

#[cfg(target_os = "macos")]
fn installation_keychain_service() -> String {
    std::env::var("CHORO_INSTALLATION_KEYCHAIN_SERVICE")
        .ok()
        .filter(|service| !service.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_INSTALLATION_KEYCHAIN_SERVICE.to_string())
}

#[cfg(not(target_os = "macos"))]
fn save_mcp_key(connection_id: Uuid, key: &str) -> Result<()> {
    write_private_file(&fallback_mcp_key_path(connection_id), key.as_bytes())
        .context("Could not save the MCP key securely")
}

#[cfg(not(target_os = "macos"))]
fn load_mcp_key(connection_id: Uuid) -> Option<String> {
    fs::read_to_string(fallback_mcp_key_path(connection_id)).ok()
}

#[cfg(not(target_os = "macos"))]
fn fallback_mcp_key_path(connection_id: Uuid) -> PathBuf {
    ide_core::AppConfig::config_root().join(format!("penpot-mcp-key-{connection_id}"))
}

#[cfg(not(target_os = "macos"))]
fn save_access_token(connection_id: Uuid, token: &str) -> Result<()> {
    write_private_file(&fallback_access_token_path(connection_id), token.as_bytes())
        .context("Could not save the Design access token securely")
}

#[cfg(not(target_os = "macos"))]
fn load_access_token(connection_id: Uuid) -> Option<String> {
    fs::read_to_string(fallback_access_token_path(connection_id)).ok()
}

#[cfg(not(target_os = "macos"))]
fn save_session_token(connection_id: Uuid, token: &str) -> Result<()> {
    write_private_file(
        &fallback_session_token_path(connection_id),
        token.as_bytes(),
    )
    .context("Could not save the Design session credential securely")
}

#[cfg(not(target_os = "macos"))]
fn load_session_token(connection_id: Uuid) -> Option<String> {
    fs::read_to_string(fallback_session_token_path(connection_id)).ok()
}

#[cfg(not(target_os = "macos"))]
fn fallback_session_token_path(connection_id: Uuid) -> PathBuf {
    ide_core::AppConfig::config_root().join(format!("penpot-session-token-{connection_id}"))
}

#[cfg(not(target_os = "macos"))]
fn load_or_create_installation_identity() -> Result<InstallationIdentity> {
    let id_path = ide_core::AppConfig::config_root().join("installation-id");
    let secret_path = ide_core::AppConfig::config_root().join("installation-secret");
    let existing_id = fs::read_to_string(&id_path)
        .ok()
        .and_then(|value| Uuid::parse_str(value.trim()).ok());
    let existing_secret = fs::read_to_string(&secret_path)
        .ok()
        .filter(|value| value.trim().len() >= 32);
    if let (Some(id), Some(secret)) = (existing_id, existing_secret) {
        return Ok(InstallationIdentity {
            id,
            secret: secret.trim().to_string(),
        });
    }
    let identity = InstallationIdentity {
        id: Uuid::new_v4(),
        secret: format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple()),
    };
    write_private_file(&id_path, identity.id.to_string().as_bytes())?;
    write_private_file(&secret_path, identity.secret.as_bytes())?;
    Ok(identity)
}

#[cfg(not(target_os = "macos"))]
fn fallback_access_token_path(connection_id: Uuid) -> PathBuf {
    ide_core::AppConfig::config_root().join(format!("penpot-access-token-{connection_id}"))
}

fn migrate_legacy_credentials(connection_id: Uuid) {
    if let Some(key) = load_keychain_secret(LEGACY_KEYCHAIN_ACCOUNT) {
        let _ = save_mcp_key(connection_id, &key);
    }
    if let Some(token) = load_keychain_secret(LEGACY_KEYCHAIN_ACCESS_TOKEN_ACCOUNT) {
        let _ = save_access_token(connection_id, &token);
    }
}

fn migrate_legacy_designs(
    store: &LocalStore,
    legacy: &PenpotConfig,
    connection: &StoredPenpotConnection,
) {
    let now = ide_core::agents::unix_now();
    for (project, legacy_designs) in &legacy.designs {
        let Some(first) = legacy_designs.first() else {
            continue;
        };
        let selected = legacy.selected_designs.get(project).copied();
        let binding = StoredProjectPenpotBinding {
            project_id: *project,
            connection_id: connection.id,
            penpot_team_id: first.team_id,
            penpot_project_id: first.penpot_project_id,
            selected_design_id: selected,
            created_at: now,
            updated_at: now,
        };
        if store.upsert_project_penpot_binding(&binding).is_err() {
            continue;
        }
        for legacy_design in legacy_designs {
            let design = PenpotDesign {
                id: legacy_design.id,
                project_id: *project,
                connection_id: connection.id,
                penpot_file_id: legacy_design.id,
                penpot_project_id: legacy_design.penpot_project_id,
                penpot_team_id: legacy_design.team_id,
                name: legacy_design.name.clone(),
                page_id: legacy_design.page_id,
                source_doc: None,
                source_task: None,
                last_synced_at: None,
                archived_at: None,
                created_at: now,
                updated_at: now,
            };
            let _ = store.upsert_penpot_design(&design);
        }
        if legacy_designs.len() == 1 {
            let shared = ide_core::DocAssistantStoreFile::load()
                .assistants
                .into_iter()
                .find(|record| {
                    record.project_id == *project
                        && record.relative_doc_path == ide_core::penpot_assistant::record_path()
                });
            if let Some(shared) = shared {
                let _ = store.import_legacy_penpot_conversation(legacy_designs[0].id, &shared);
            } else {
                let _ = store.ensure_current_penpot_conversation(legacy_designs[0].id);
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn write_private_file(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut options = fs::OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    use std::io::Write as _;
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_bootstrap_accepts_optional_starter_design() {
        let without_starter: ManagedPenpotBootstrap = serde_json::from_value(json!({
            "instanceUrl": "https://design.example",
            "mcpUrl": "https://design.example/mcp/stream",
            "profileId": Uuid::from_u128(1),
            "profileEmail": "user@example.test",
            "teamId": Uuid::from_u128(2),
            "projectId": Uuid::from_u128(3),
            "apiToken": "api",
            "mcpKey": "mcp",
            "sessionToken": "session"
        }))
        .unwrap();
        assert!(without_starter.starter_design.is_none());

        let with_starter: ManagedPenpotBootstrap = serde_json::from_value(json!({
            "instanceUrl": "https://design.example",
            "mcpUrl": "https://design.example/mcp/stream",
            "profileId": Uuid::from_u128(1),
            "profileEmail": "user@example.test",
            "teamId": Uuid::from_u128(2),
            "projectId": Uuid::from_u128(3),
            "apiToken": "api",
            "mcpKey": "mcp",
            "sessionToken": "session",
            "starterDesign": {
                "fileId": Uuid::from_u128(4),
                "projectId": Uuid::from_u128(3),
                "teamId": Uuid::from_u128(2),
                "name": "Welcome to Choro Design",
                "pageId": Uuid::from_u128(5),
                "templateVersion": "ignored-by-desktop"
            }
        }))
        .unwrap();
        let starter = with_starter.starter_design.unwrap();
        assert_eq!(starter.file_id, Uuid::from_u128(4));
        assert_eq!(starter.page_id, Some(Uuid::from_u128(5)));
    }

    #[test]
    fn copied_remote_url_is_split_from_its_key() {
        let (config, key) = PenpotConfig::normalized(
            "https://design.penpot.app/",
            "https://design.penpot.app/mcp/stream?userToken=secret-key&mode=remote",
        )
        .unwrap();

        assert_eq!(config.instance_url, "https://design.penpot.app");
        assert_eq!(key.as_deref(), Some("secret-key"));
        assert!(!config.mcp_url.contains("secret-key"));
        assert_eq!(
            config.full_mcp_url(key.as_deref()).unwrap(),
            "https://design.penpot.app/mcp/stream?mode=remote&userToken=secret-key"
        );
    }

    #[test]
    fn local_mcp_does_not_require_a_key() {
        let (config, key) =
            PenpotConfig::normalized("https://design.penpot.app", "http://localhost:4401/mcp")
                .unwrap();

        assert!(key.is_none());
        assert!(config.is_local());
        assert_eq!(
            config.full_mcp_url(None).unwrap(),
            "http://localhost:4401/mcp"
        );
    }

    #[test]
    fn decodes_penpot_transit_maps_and_identifiers() {
        let decoded = decode_penpot_transit(json!([
            "^ ",
            "~:id",
            "~u00000000-0000-0000-0000-000000000001",
            "~:default-team-id",
            "~u00000000-0000-0000-0000-000000000002",
            "~:props",
            ["^ ", "~:mcp-enabled", true]
        ]));

        assert_eq!(
            decoded.get("id").and_then(serde_json::Value::as_str),
            Some("00000000-0000-0000-0000-000000000001")
        );
        assert_eq!(
            decoded
                .get("defaultTeamId")
                .and_then(serde_json::Value::as_str),
            Some("00000000-0000-0000-0000-000000000002")
        );
        assert_eq!(
            decoded
                .get("props")
                .and_then(|props| props.get("mcpEnabled"))
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );
    }

    #[test]
    fn cloud_profile_uses_both_default_workspace_ids() {
        let team_id = Uuid::from_u128(1);
        let project_id = Uuid::from_u128(2);
        let mut calls = Vec::new();

        let resolved = resolve_penpot_workspace_with("access-token", |method, _| {
            calls.push(method.to_string());
            match method {
                "get-profile" => Ok(json!({
                    "id": Uuid::from_u128(3),
                    "defaultTeamId": team_id,
                    "defaultProjectId": project_id,
                })),
                _ => panic!("unexpected Design service method: {method}"),
            }
        })
        .unwrap();

        assert_eq!(resolved.team_id, team_id);
        assert_eq!(resolved.project_id, project_id);
        assert_eq!(calls, ["get-profile"]);
    }

    #[test]
    fn self_hosted_profile_resolves_team_from_default_project() {
        let team_id = Uuid::from_u128(10);
        let project_id = Uuid::from_u128(11);
        let mut calls = Vec::new();

        let resolved = resolve_penpot_workspace_with("access-token", |method, body| {
            calls.push(method.to_string());
            match method {
                "get-profile" => Ok(json!({ "defaultProjectId": project_id })),
                "get-project" => {
                    assert_eq!(json_uuid(&body, "id"), Some(project_id));
                    Ok(json!({ "id": project_id, "teamId": team_id }))
                }
                _ => panic!("unexpected Design service method: {method}"),
            }
        })
        .unwrap();

        assert_eq!(resolved.team_id, team_id);
        assert_eq!(resolved.project_id, project_id);
        assert_eq!(calls, ["get-profile", "get-project"]);
    }

    #[test]
    fn profile_without_defaults_uses_an_accessible_project() {
        let team_id = Uuid::from_u128(20);
        let project_id = Uuid::from_u128(21);

        let resolved = resolve_penpot_workspace_with("access-token", |method, _| match method {
            "get-profile" => Ok(json!({ "id": Uuid::from_u128(22) })),
            "get-all-projects" => Ok(json!([
                { "id": project_id, "teamId": team_id }
            ])),
            _ => panic!("unexpected Design service method: {method}"),
        })
        .unwrap();

        assert_eq!(resolved.team_id, team_id);
        assert_eq!(resolved.project_id, project_id);
    }

    #[test]
    fn all_projects_prefers_default_team_default_project_then_default_project() {
        let first_team = Uuid::from_u128(30);
        let first_project = Uuid::from_u128(31);
        let default_team = Uuid::from_u128(32);
        let default_project = Uuid::from_u128(33);
        let preferred_team = Uuid::from_u128(34);
        let preferred_project = Uuid::from_u128(35);

        let resolved = resolve_penpot_workspace_with("access-token", |method, _| match method {
            "get-profile" => Ok(json!({})),
            "get-all-projects" => Ok(json!([
                {
                    "id": first_project,
                    "teamId": first_team,
                },
                {
                    "id": default_project,
                    "teamId": default_team,
                    "isDefault": true,
                },
                {
                    "id": preferred_project,
                    "teamId": preferred_team,
                    "isDefaultTeam": true,
                    "isDefault": true,
                }
            ])),
            _ => panic!("unexpected Design service method: {method}"),
        })
        .unwrap();

        assert_eq!(resolved.team_id, preferred_team);
        assert_eq!(resolved.project_id, preferred_project);
    }

    #[test]
    fn empty_all_projects_falls_back_to_default_team_and_project() {
        let first_team = Uuid::from_u128(40);
        let default_team = Uuid::from_u128(41);
        let first_project = Uuid::from_u128(42);
        let default_project = Uuid::from_u128(43);

        let resolved = resolve_penpot_workspace_with("access-token", |method, body| match method {
            "get-profile" => Ok(json!({})),
            "get-all-projects" => Ok(json!([])),
            "get-teams" => Ok(json!([
                { "id": first_team },
                { "id": default_team, "isDefault": true }
            ])),
            "get-projects" => {
                assert_eq!(json_uuid(&body, "teamId"), Some(default_team));
                Ok(json!([
                    { "id": first_project },
                    { "id": default_project, "isDefault": true }
                ]))
            }
            _ => panic!("unexpected Design service method: {method}"),
        })
        .unwrap();

        assert_eq!(resolved.team_id, default_team);
        assert_eq!(resolved.project_id, default_project);
    }

    #[test]
    fn account_without_accessible_workspace_gets_actionable_error() {
        let team_id = Uuid::from_u128(50);
        let error = resolve_penpot_workspace_with("access-token", |method, _| match method {
            "get-profile" => Ok(json!({})),
            "get-all-projects" => Ok(json!([])),
            "get-teams" => Ok(json!([{ "id": team_id }])),
            "get-projects" => Ok(json!([])),
            _ => panic!("unexpected Design service method: {method}"),
        })
        .unwrap_err();

        assert_eq!(error, NO_WORKSPACE_ERROR);
    }

    #[test]
    fn design_creation_uses_stored_workspace_without_loading_profile() {
        let team_id = Uuid::from_u128(60);
        let project_id = Uuid::from_u128(61);
        let file_id = Uuid::from_u128(62);
        let page_id = Uuid::from_u128(63);
        let mut calls = Vec::new();

        let design = create_penpot_design_with(
            "access-token",
            "Stored workspace design",
            Some(team_id),
            Some(project_id),
            |method, body| {
                calls.push(method.to_string());
                match method {
                    "create-file" => {
                        assert_eq!(json_uuid(&body, "projectId"), Some(project_id));
                        Ok(json!({
                            "id": file_id,
                            "name": "Stored workspace design",
                            "pages": [{ "id": page_id }],
                        }))
                    }
                    _ => panic!("unexpected Design service method: {method}"),
                }
            },
        )
        .unwrap();

        assert_eq!(calls, ["create-file"]);
        assert_eq!(design.team_id, team_id);
        assert_eq!(design.penpot_project_id, project_id);
        assert_eq!(design.id, file_id);
        assert_eq!(design.page_id, Some(page_id));
        assert!(design.resolved_workspace.is_none());
    }

    #[test]
    fn design_deletion_targets_the_exact_remote_file() {
        let file_id = Uuid::from_u128(64);
        let mut calls = Vec::new();

        delete_penpot_design_with(file_id, |method, body| {
            calls.push(method.to_string());
            assert_eq!(json_uuid(&body, "id"), Some(file_id));
            Ok(json!({}))
        })
        .unwrap();

        assert_eq!(calls, ["delete-file"]);
    }

    #[test]
    fn successful_empty_design_service_response_is_valid() {
        assert_eq!(
            decode_penpot_response_body(" \n ").unwrap(),
            serde_json::Value::Null
        );
    }

    #[test]
    fn design_deletion_accepts_an_already_missing_remote_file() {
        let file_id = Uuid::from_u128(65);

        delete_penpot_design_with(file_id, |method, body| {
            assert_eq!(method, "delete-file");
            assert_eq!(json_uuid(&body, "id"), Some(file_id));
            Err("Design service returned HTTP 404: object not found".to_string())
        })
        .unwrap();
    }

    #[test]
    fn workspace_url_uses_resolved_team_file_and_page_ids() {
        let team_id = Uuid::from_u128(70);
        let penpot_project_id = Uuid::from_u128(71);
        let file_id = Uuid::from_u128(72);
        let page_id = Uuid::from_u128(73);
        let config = PenpotConfig::normalized(
            "https://self-hosted.penpot.example",
            "https://self-hosted.penpot.example/mcp/stream",
        )
        .unwrap()
        .0;
        let now = ide_core::agents::unix_now();
        let design = PenpotDesign {
            id: Uuid::from_u128(74),
            project_id: ProjectId(Uuid::from_u128(75)),
            connection_id: Uuid::from_u128(76),
            penpot_file_id: file_id,
            penpot_project_id,
            penpot_team_id: team_id,
            name: "Self-hosted design".to_string(),
            page_id: Some(page_id),
            source_doc: None,
            source_task: None,
            last_synced_at: Some(now),
            archived_at: None,
            created_at: now,
            updated_at: now,
        };

        let url = config.workspace_url(&design).unwrap();

        assert!(url.starts_with("https://self-hosted.penpot.example/"));
        assert!(url.contains(&format!("team-id={team_id}")));
        assert!(url.contains(&format!("file-id={file_id}")));
        assert!(url.contains(&format!("page-id={page_id}")));
        assert_eq!(design.penpot_project_id, penpot_project_id);
    }

    #[test]
    fn external_mcp_workspace_url_preserves_exact_design_route() {
        let team_id = Uuid::from_u128(80);
        let file_id = Uuid::from_u128(81);
        let page_id = Uuid::from_u128(82);
        let config = PenpotConfig::normalized(
            "https://design.example",
            "https://design.example/mcp/stream",
        )
        .unwrap()
        .0;
        let design = PenpotDesign {
            id: Uuid::from_u128(83),
            project_id: ProjectId(Uuid::from_u128(84)),
            connection_id: Uuid::from_u128(85),
            penpot_file_id: file_id,
            penpot_project_id: Uuid::from_u128(86),
            penpot_team_id: team_id,
            name: "External MCP".to_string(),
            page_id: Some(page_id),
            source_doc: None,
            source_task: None,
            last_synced_at: None,
            archived_at: None,
            created_at: 1,
            updated_at: 1,
        };

        let url = config.external_mcp_workspace_url(&design).unwrap();

        assert!(url.contains(&format!("team-id={team_id}")));
        assert!(url.contains(&format!("file-id={file_id}")));
        assert!(url.contains(&format!("page-id={page_id}")));
        assert!(url.ends_with("&choro-mcp=connect"));
    }

    #[test]
    fn managed_service_matching_uses_the_http_origin() {
        assert!(same_http_origin(
            "https://design.example/workspace",
            "https://design.example/choro/penpot/v1/bootstrap"
        ));
        assert!(same_http_origin(
            "http://localhost:9001/workspace",
            "http://localhost:9001/choro/penpot/v1/bootstrap"
        ));
        assert!(!same_http_origin(
            "https://design.example",
            "https://other.example/choro/penpot/v1/bootstrap"
        ));
        assert!(!same_http_origin(
            "http://design.example",
            "https://design.example/choro/penpot/v1/bootstrap"
        ));
    }

    #[test]
    fn automatic_provisioning_preserves_explicit_cloud_connections() {
        let now = ide_core::agents::unix_now();
        let cloud = StoredPenpotConnection {
            id: Uuid::new_v4(),
            instance_url: "https://design.penpot.app".to_string(),
            mcp_url: "https://design.penpot.app/mcp/stream".to_string(),
            profile_id: None,
            profile_email: None,
            default_team_id: None,
            default_project_id: None,
            is_active: true,
            verified_at: Some(now),
            created_at: now,
            updated_at: now,
        };

        assert_eq!(
            design_provider_for_connection(Some(&cloud)),
            DesignProvider::PenpotCloud
        );
        assert!(!should_auto_provision_connection(Some(&cloud)));

        let mut managed = cloud;
        managed.instance_url = managed_provisioning_url();
        assert_eq!(
            design_provider_for_connection(Some(&managed)),
            DesignProvider::Choro
        );
        assert!(should_auto_provision_connection(Some(&managed)));
        assert!(should_auto_provision_connection(None));
    }

    #[test]
    fn credentials_are_redacted_from_every_workspace_error() {
        let access_token = "penpot-access-token-secret";
        let mcp_key = "penpot-mcp-key-secret";
        let resolver_error =
            resolve_penpot_workspace_with(access_token, |_, _| Err(access_token.to_string()))
                .unwrap_err();
        let combined_error = redact_penpot_error(
            &format!("API {access_token}; MCP {mcp_key}"),
            [access_token, mcp_key],
        );

        for error in [resolver_error, combined_error] {
            assert!(!error.contains(access_token));
            assert!(!error.contains(mcp_key));
        }
    }
}
