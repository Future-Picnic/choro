//! Image overview with one session-scoped, isolated inline editor.
use super::*;
use anyhow::Context as _;
use ide_core::studio::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Mutex, OnceLock},
};

const MAX_MESSAGE: usize = 128 * 1024;
const IMAGE_BUDGET: usize = 32 * 1024 * 1024;
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(deny_unknown_fields)]
pub(super) struct PreviewRequest {
    pub screen_id: Uuid,
    pub content_key: String,
    pub tier: u32,
}
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub(super) enum Action {
    Ready,
    CommentToggle,
    CommentsRead,
    CommentDraft { dirty: bool },
    CommentEdit { revision: u64, operation: StudioCommentOperation },
    CommentSend { id: Uuid, revision: u64 },
    CommentFocus { id: Uuid },
    Failed {
        error: String,
    },
    Select {
        screen_id: Option<Uuid>,
    },
    Open {
        screen_id: Uuid,
    },
    CloseEditor,
    EditorFailed { screen_id: Uuid, error: String },
    Camera {
        viewport: StudioCanvasViewport,
    },
    Position {
        screen_id: Uuid,
        position: StudioCanvasPoint,
    },
    Resize {
        screen_id: Uuid,
        width: u32,
        height: u32,
        x: f64,
        y: f64,
        revision: u64,
        fingerprint: String,
    },
    Previews {
        requests: Vec<PreviewRequest>,
    },
    Flushed,
    SelectSection {
        section_id: Option<Uuid>,
    },
    /// A drag or menu move; carries the interaction-start revision.
    MoveScreen {
        screen_id: Uuid,
        section_id: Option<Uuid>,
        before_screen_id: Option<Uuid>,
        position: Option<StudioCanvasPoint>,
        revision: u64,
        fingerprint: String,
    },
    ReorderSections {
        section_ids: Vec<Uuid>,
        revision: u64,
        fingerprint: String,
    },
    /// A canvas context-menu command for exactly one screen or section.
    ContextAction {
        screen_id: Option<Uuid>,
        section_id: Option<Uuid>,
        action: String,
    },
}
const SCREEN_ACTIONS: [&str; 6] = ["rename", "duplicate", "archive", "up", "down", "new-section"];
const SECTION_ACTIONS: [&str; 7] = ["rename", "add-screen", "earlier", "later", "fit", "ungroup", "edit"];
#[derive(Debug, Deserialize)]
pub(super) struct Message {
    pub session: Uuid,
    pub request_id: Uuid,
    #[serde(flatten)]
    pub action: Action,
}
static MESSAGES: OnceLock<Mutex<VecDeque<Message>>> = OnceLock::new();
pub(super) fn enqueue(raw: &str, session: Uuid) -> bool {
    if raw.len() > MAX_MESSAGE {
        return false;
    }
    let Ok(message) = serde_json::from_str::<Message>(raw) else {
        return false;
    };
    if message.session != session || message.request_id.is_nil() {
        return false;
    }
    let valid = match &message.action {
        Action::CommentEdit { operation, .. } => operation.validate().is_ok(),
        Action::CommentSend { id, .. } | Action::CommentFocus { id } => !id.is_nil(),
        Action::Camera { viewport } => viewport.validate().is_ok(),
        Action::Position { position, .. } => position.validate().is_ok(),
        Action::Resize {
            width,
            height,
            x,
            y,
            fingerprint,
            ..
        } => {
            (240..=3840).contains(width)
                && (240..=4096).contains(height)
                && StudioCanvasPoint { x: *x, y: *y }.validate().is_ok()
                && fingerprint.len() <= 128
        }
        Action::Previews { requests } => {
            requests.len() <= 200
                && requests
                    .iter()
                    .all(|r| [256, 512, 1024, 2048].contains(&r.tier) && r.content_key.len() <= 128)
        }
        Action::Failed { error } | Action::EditorFailed { error, .. } => error.len() <= 2048,
        Action::MoveScreen { position, fingerprint, before_screen_id, screen_id, .. } => {
            fingerprint.len() <= 128
                && position.is_none_or(|p| p.validate().is_ok())
                && *before_screen_id != Some(*screen_id)
        }
        Action::ReorderSections { section_ids, fingerprint, .. } => {
            section_ids.len() <= MAX_SECTIONS && fingerprint.len() <= 128
        }
        Action::ContextAction { screen_id, section_id, action } => match (screen_id, section_id) {
            (Some(_), None) => SCREEN_ACTIONS.contains(&action.as_str()),
            (None, Some(_)) => SECTION_ACTIONS.contains(&action.as_str()),
            _ => false,
        },
        _ => true,
    };
    if !valid {
        return false;
    }
    let mut queue = MESSAGES
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if queue.len() >= 256 {
        return false;
    }
    queue.push_back(message);
    true
}
struct ImageEntry {
    key: Uuid,
    content: String,
    tier: u32,
    bytes: Arc<Vec<u8>>,
    width: u32,
    height: u32,
}
#[derive(Default)]
struct Images {
    entries: VecDeque<ImageEntry>,
    bytes: usize,
    grants: HashMap<Uuid, HashSet<Uuid>>,
}
static IMAGES: OnceLock<Mutex<Images>> = OnceLock::new();
fn images() -> std::sync::MutexGuard<'static, Images> {
    IMAGES
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}
fn cached(session: Uuid, request: &PreviewRequest) -> Option<Value> {
    let mut cache = images();
    let i = cache
        .entries
        .iter()
        .position(|e| e.content == request.content_key && e.tier == request.tier)?;
    let entry = cache.entries.remove(i)?;
    let reply = json!({"session":session,"type":"preview","screen_id":request.screen_id,"content_key":request.content_key,"tier":request.tier,"key":entry.key,"width":entry.width,"height":entry.height});
    cache.grants.entry(session).or_default().insert(entry.key);
    cache.entries.push_back(entry);
    Some(reply)
}
fn insert(request: &PreviewRequest, bytes: Vec<u8>) -> anyhow::Result<()> {
    let (width, height) = super::studio_editor::png_size(&bytes)
        .ok_or_else(|| anyhow::anyhow!("Invalid preview image"))?;
    anyhow::ensure!(
        width <= 2048 && height <= 2048 && bytes.len() <= IMAGE_BUDGET,
        "Preview exceeds image limits"
    );
    let mut cache = images();
    while cache.bytes + bytes.len() > IMAGE_BUDGET {
        let Some(old) = cache.entries.pop_front() else {
            break;
        };
        cache.bytes -= old.bytes.len();
        for grants in cache.grants.values_mut() {
            grants.remove(&old.key);
        }
    }
    cache.bytes += bytes.len();
    cache.entries.push_back(ImageEntry {
        key: Uuid::new_v4(),
        content: request.content_key.clone(),
        tier: request.tier,
        bytes: Arc::new(bytes),
        width,
        height,
    });
    Ok(())
}
pub(super) fn image_bytes(session: Uuid, key: Uuid) -> Option<Arc<Vec<u8>>> {
    let cache = images();
    if !cache.grants.get(&session)?.contains(&key) {
        return None;
    }
    cache
        .entries
        .iter()
        .find(|e| e.key == key)
        .map(|e| e.bytes.clone())
}
fn revoke(session: Uuid) {
    images().grants.remove(&session);
}

pub(super) struct Runtime {
    pub layout: StudioCanvasState,
    pub session: Uuid,
    pub html: Option<String>,
    pub ready: bool,
    pub comment_draft: bool,
    pub comments_busy: bool,
    comment_request: Option<Uuid>,
    pub focus_screen: Option<Uuid>,
    pub pending_fit_section: Option<Uuid>,
    fit_all_requested: bool,
    visible: bool,
    flush_request: Option<Uuid>,
    navigation_ready: bool,
    pub corrupt: bool,
    pub failed: bool,
    busy: bool,
    worker: Option<Uuid>,
    retry_at: std::time::Instant,
    desired: Vec<PreviewRequest>,
    keys: HashMap<Uuid, String>,
    delivered: HashSet<PreviewRequest>,
    seen: VecDeque<Uuid>,
    fingerprint: String,
    theme: String,
    activity: String,
    designing: BTreeMap<Uuid, StudioScreenActivity>,
    designing_checked: Option<std::time::Instant>,
    opened: std::time::Instant,
    initial: bool,
}
impl Runtime {
    pub fn new(store: &StudioStore, design: &StudioDesign) -> (Self, Option<String>) {
        let (mut layout,notice,corrupt)=match store.canvas_state(design.manifest.id){Ok(s)=>(s,None,false),Err(e)=>(Default::default(),Some(format!("Saved canvas layout could not be read; using a temporary layout. Arrange can recover it while preserving the original. {e}")),true)};
        let initial = layout.positions.is_empty();
        // New workspaces open on Canvas; retain each design's explicit view choice.
        layout.reconcile_design(&design.manifest);
        (
            Self {
                layout,
                session: Uuid::new_v4(),
                html: None,
                ready: false,
                comment_draft: false,
                comments_busy: false,
                comment_request: None,
                focus_screen: None,
                pending_fit_section: None,
                fit_all_requested: false,
                visible: false,
                flush_request: None,
                navigation_ready: false,
                corrupt,
                failed: false,
                busy: false,
                worker: None,
                retry_at: std::time::Instant::now(),
                desired: vec![],
                keys: HashMap::new(),
                delivered: HashSet::new(),
                seen: VecDeque::new(),
                fingerprint: String::new(),
                theme: String::new(),
                activity: String::new(),
                designing: BTreeMap::new(),
                designing_checked: None,
                opened: std::time::Instant::now(),
                initial,
            },
            notice,
        )
    }
    pub fn invalidate_metadata(&mut self) { self.fingerprint.clear(); }
    pub fn comment_surface_pending(&self) -> bool { self.comment_draft || self.comments_busy }
    fn begin_comment_request(&mut self) -> Uuid {
        let owner = Uuid::new_v4();
        self.comment_request = Some(owner);
        self.comments_busy = true;
        owner
    }
    fn finish_comment_request(&mut self, owner: Uuid) -> bool {
        if self.comment_request != Some(owner) { return false; }
        self.comment_request = None;
        self.comments_busy = false;
        true
    }
    pub fn navigation_pending(&self) -> bool { self.flush_request.is_some() }
    pub fn cancel_navigation(&mut self) { self.flush_request=None;self.navigation_ready=false; }
    pub fn active(&self) -> bool {
        !self.failed && self.layout.overview_mode != StudioOverviewMode::Grid
    }
    fn save(&self, store: &StudioStore, id: Uuid) -> anyhow::Result<()> {
        if self.corrupt {
            return Ok(());
        }
        store.save_canvas_state(id, &self.layout)
    }
    pub fn leave(&mut self) {
        revoke(self.session);
        super::studio_editor::revoke_inline(self.session);
        self.session = Uuid::new_v4();
        self.html = None;
        self.ready = false;
        self.comment_draft = false;
        self.comments_busy = false;
        self.comment_request = None;
        self.focus_screen = None;
        self.desired.clear();
        self.delivered.clear();
        self.fingerprint.clear();
        self.activity.clear();
        self.designing.clear();
        self.designing_checked = None;
        self.opened = std::time::Instant::now();
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        revoke(self.session);
        super::studio_editor::revoke_inline(self.session);
    }
}
fn content_key(store: &StudioStore, design: &StudioDesign, id: Uuid) -> String {
    // Existing screenshot key includes document, viewport, effective system and assets.
    format!(
        "canvas-v1-{}",
        store
            .thumbnail_path(design, id)
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
    )
}
/// Screens plus freshly derived agent activity, for replies outside the refresh path.
fn screens_value(store: &StudioStore, design: &StudioDesign) -> Value {
    metadata(store, design, &designing_screens(store, design))
}
fn metadata(
    store: &StudioStore,
    design: &StudioDesign,
    activity: &BTreeMap<Uuid, StudioScreenActivity>,
) -> Value {
    json!(design.manifest.screens.iter().map(|s|json!({"id":s.id,"name":s.name,"width":s.width,"height":s.height,"archived":s.archived,"content_key":content_key(store,design,s.id),"activity":activity.get(&s.id).map(|state|state.as_str())})).collect::<Vec<_>>())
}
/// Authoritative section metadata and geometry, in sidebar order.
fn sections_value(manifest: &StudioDesignManifest, board: &StudioBoardGeometry) -> Value {
    json!(board.sections.iter().filter_map(|b| {
        let section = manifest.section(b.id)?;
        Some(json!({"id":section.id,"name":section.name,"direction":section.direction,"gap":section.gap,
            "title_style":section.title_style,"header_alignment":section.header_alignment,
            "screen_ids":section.screen_ids,"active_screen_ids":b.active_screen_ids,
            "x":b.x,"y":b.y,"width":b.width,"height":b.height,"header_height":b.header_height}))
    }).collect::<Vec<_>>())
}
/// Personal layout with grouped screens at their derived board positions.
fn layout_value(layout: &StudioCanvasState, manifest: &StudioDesignManifest) -> (Value, Value) {
    let board = layout.board(manifest);
    let mut value = serde_json::to_value(layout).unwrap_or_default();
    value["positions"] = json!(layout.effective_positions(&board));
    value["selected_section_id"] = json!(layout.selected_section_id);
    (value, sections_value(manifest, &board))
}
fn resize_result(s: &super::studio::StudioWorkspace, request: Uuid, screen: Uuid, error: Option<String>) -> Value {
    let mut reply = move_result(s, request, error);
    reply["type"] = json!("resize-result");
    reply["screen_id"] = json!(screen);
    reply
}
pub(super) fn move_result(s: &super::studio::StudioWorkspace, request: Uuid, error: Option<String>) -> Value {
    let (layout, sections) = layout_value(&s.canvas.layout, &s.design.manifest);
    json!({"session":s.canvas.session,"type":"move-result","request_id":request,"error":error,
        "revision":s.design.manifest.revision,"fingerprint":s.design.fingerprint,
        "screens":screens_value(&s.store,&s.design),"sections":sections,"positions":layout["positions"],
        "arrangement":s.design.manifest.section_layout.direction})
}
fn document(bootstrap: Value) -> String {
    let data = bootstrap
        .to_string()
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026");
    format!("<!doctype html><html><head><meta charset=\"utf-8\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src choro-canvas-image: data:; font-src data:; connect-src 'none'; frame-src about: data:; base-uri 'none'; form-action 'none'\"><style>{}</style></head><body><div id=\"root\"></div><script>window.__CHORO_CANVAS__={};{}</script></body></html>",include_str!("../../../web/studio-canvas/dist/canvas.css"),data,include_str!("../../../web/studio-canvas/dist/canvas.js").replace("</script","<\\/script"))
}
impl CenterArea {
    /// Selection changes never read documents or regenerate screen preview keys.
    pub(super) fn studio_canvas_selection(&self, cx: &App) {
        let Some(s) = self.studio.as_ref() else { return; };
        if !s.canvas.ready { return; }
        self.web_host.read(cx).canvas_reply(&json!({"session":s.canvas.session,
            "type":"selection","screen_id":s.canvas.layout.selected_screen_id,
            "section_id":s.canvas.layout.selected_section_id}));
    }
    pub(super) fn focus_studio_canvas_screen(&mut self, cx: &App) {
        let Some(s) = self.studio.as_mut() else { return; };
        let Some(screen) = s.canvas.focus_screen else { return; };
        // Keep the request until both startup and the previous editor's save
        // have completed. Re-selecting the current screen also recenters it.
        if !s.canvas.ready || s.inline_screen != Some(screen) || s.editor_html.is_none() {
            return;
        }
        s.canvas.focus_screen = None;
        self.web_host.read(cx).canvas_reply(&json!({
            "session":s.canvas.session,"type":"command",
            "command":"focus-screen","screen_id":screen
        }));
    }
    pub(super) fn studio_canvas_active(&self) -> bool {
        self.studio.as_ref().is_some_and(|s| {
            s.screen.is_none() && !s.design.manifest.system_workspace && s.canvas.active()
        })
    }
    pub(super) fn refresh_studio_canvas(&mut self, cx: &App) {
        let Some(s) = self.studio.as_mut() else {
            return;
        };
        if s.screen.is_some() || s.design.manifest.system_workspace || !s.canvas.active() {
            return;
        }
        let theme = super::studio_editor::web_theme(cx);
        let theme_key = theme.to_string();
        // Agent progress moves without touching the design fingerprint, so it is
        // polled here rather than derived from it. This runs on every frame;
        // re-reading the scope directory that often would be wasteful.
        let elapsed = |t: std::time::Instant| t.elapsed() >= Duration::from_millis(400);
        if s.canvas.designing_checked.is_none_or(|t| elapsed(t)) {
            s.canvas.designing = designing_screens(&s.store, &s.design);
            s.canvas.designing_checked = Some(std::time::Instant::now());
        }
        let designing = s.canvas.designing.clone();
        let activity_key = activity_key(&designing);
        if s.canvas.html.is_some()
            && (!s.canvas.ready
                || (s.canvas.fingerprint == s.design.fingerprint
                    && s.canvas.theme == theme_key
                    && s.canvas.activity == activity_key))
        {
            return;
        }
        s.canvas.layout.reconcile_design(&s.design.manifest);
        let screens = metadata(&s.store, &s.design, &designing);
        let (layout, sections) = layout_value(&s.canvas.layout, &s.design.manifest);
        let arrangement = s.design.manifest.section_layout.direction;
        s.canvas.keys = screens
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|screen| {
                Some((
                    screen["id"].as_str()?.parse().ok()?,
                    screen["content_key"].as_str()?.to_string(),
                ))
            })
            .collect();
        if s.canvas.html.is_none() {
            let theme = super::studio_editor::web_theme(cx);
            s.canvas.html = Some(document(
                json!({"session":s.canvas.session,"revision":s.design.manifest.revision,"fingerprint":s.design.fingerprint,"screens":screens,"sections":sections,"arrangement":arrangement,"layout":layout,"theme":theme,"fit_initial":s.canvas.initial,"comment_mode":s.comment_mode}),
            ));
            s.canvas.initial = false;
            s.canvas.opened = std::time::Instant::now();
        } else if s.canvas.ready {
            let value = json!({"session":s.canvas.session,"type":"state","revision":s.design.manifest.revision,"fingerprint":s.design.fingerprint,"screens":screens,"sections":sections,"arrangement":arrangement,"layout":layout,"theme":theme,"comment_mode":s.comment_mode});
            self.web_host.read(cx).canvas_reply(&value);
        }
        s.canvas.fingerprint = s.design.fingerprint.clone();
        s.canvas.theme = theme_key;
        s.canvas.activity = activity_key;
    }
    pub(super) fn studio_canvas_command(&mut self, command: &str, cx: &mut Context<Self>) {
        let Some(s) = self.studio.as_mut() else {
            return;
        };
        if command == "fit-all" && !s.canvas.ready {s.canvas.fit_all_requested=true;return;}
        if command == "arrange" {
            s.canvas.layout.arrange_design(&s.design.manifest);
            let result = if s.canvas.corrupt {
                s.store
                    .recover_canvas_state(s.design.manifest.id, &s.canvas.layout)
            } else {
                s.canvas.save(&s.store, s.design.manifest.id)
            };
            match result {
                Ok(()) => s.canvas.corrupt = false,
                Err(e) => s.error = Some(format!("Could not save canvas arrangement: {e}")),
            }
            s.canvas.fingerprint.clear();
            self.refresh_studio_canvas(cx);
            self.studio_canvas_command("fit-all", cx);
            return;
        }
        if command == "refresh" {
            s.canvas.delivered.clear();
        }
        let value = json!({"session":s.canvas.session,"type":"command","command":command});
        self.web_host
            .update(cx, |host, _| host.canvas_reply(&value));
        cx.notify();
    }
    pub(super) fn studio_overview_mode(
        &mut self,
        mode: StudioOverviewMode,
        cx: &mut Context<Self>,
    ) {
        if self.studio.as_ref().is_some_and(|s| s.comment_mode) { return; }
        // Canvas and Focus share one live editor. A view change must not flush,
        // reconstruct the iframe, or clear its selection and undo stack.
        let spatial_switch = self.studio_canvas_active() && mode != StudioOverviewMode::Grid;
        if spatial_switch {
            let Some(s) = self.studio.as_mut() else { return; };
            if s.canvas.layout.overview_mode == mode { return; }
            s.canvas.layout.overview_mode = mode;
            if mode == StudioOverviewMode::Focus {
                let screen = s.inline_screen
                    .or(s.canvas.layout.selected_screen_id)
                    .or_else(|| s.design.manifest.screens.iter().find(|p| !p.archived).map(|p| p.id));
                s.canvas.layout.select_screen(screen);
            }
            s.canvas.invalidate_metadata();
            if let Err(e) = s.canvas.save(&s.store, s.design.manifest.id) { s.error = Some(e.to_string()); }
            let target = s.canvas.layout.selected_screen_id;
            let needs_editor = mode == StudioOverviewMode::Focus && s.inline_screen.is_none();
            self.refresh_studio_canvas(cx);
            if needs_editor { self.studio_inline_select(target, cx); }
            cx.notify();
            return;
        }
        if self.studio.as_ref().is_some_and(|s|s.screen.is_some())
            && self.defer_studio_navigation(move |this,cx|this.studio_overview_mode(mode,cx),cx) { return; }
        if self.defer_canvas_navigation(move |this, cx| this.studio_overview_mode(mode, cx), cx) {
            return;
        }
        let Some(s) = self.studio.as_mut() else {
            return;
        };
        s.canvas.layout.overview_mode = mode;
        s.screen = None;
        s.inline_screen = None;
        s.inline_flush = None;
        s.editor_html = None;
        s.selected_element = None;
        s.prototype = false;
        s.preview_mode = false;
        s.viewing_size = None;
        s.prototype_history.clear();
        s.canvas.failed = false;
        if let Err(e) = s.canvas.save(&s.store, s.design.manifest.id) {
            s.error = Some(format!("Could not save overview preference: {e}"));
        }
        s.canvas.leave();
        let target = if mode == StudioOverviewMode::Focus {
            s.canvas.layout.selected_screen_id.or_else(||s.design.manifest.screens.iter().find(|p|!p.archived).map(|p|p.id))
        } else { None };
        self.refresh_studio_canvas(cx);
        if target.is_some() { self.studio_inline_select(target,cx); }
        cx.notify();
    }
    pub(super) fn defer_canvas_navigation(
        &mut self,
        action: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> bool {
        if let Some(s) = self.studio.as_mut().filter(|s| s.comment_mode && (s.canvas.comment_draft || s.canvas.comments_busy)) {
            s.error = Some("Post or cancel your comment before leaving Comments. Wait for any pending save to finish.".into());
            cx.notify();
            return true;
        }
        if !self.studio_canvas_active() {
            return false;
        }
        let Some(s) = self.studio.as_mut().filter(|s| s.canvas.ready) else {
            return false;
        };
        s.pending_navigation = Some(Box::new(action));
        let request = Uuid::new_v4();
        s.canvas.flush_request = Some(request);
        let value = json!({"session":s.canvas.session,"type":"command","command":"flush","request_id":request});
        self.web_host
            .update(cx, |host, _| host.canvas_reply(&value));
        true
    }
    pub(super) fn flush_studio_canvas(&mut self) {
        if let Some(s) = self.studio.as_mut() {
            if let Err(e) = s.canvas.save(&s.store, s.design.manifest.id) {
                s.error = Some(format!("Could not save canvas layout: {e}"));
            }
            s.canvas.leave();
        }
    }
    pub(super) fn process_studio_canvas(&mut self, visible: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(s) = self.studio.as_mut() {
            if visible && !s.canvas.visible && s.canvas.ready {
                let value =
                    json!({"session":s.canvas.session,"type":"command","command":"refresh"});
                self.web_host
                    .update(cx, |host, _| host.canvas_reply(&value));
            }
            s.canvas.visible = visible;
        }
        let messages = std::mem::take(
            &mut *MESSAGES
                .get_or_init(Default::default)
                .lock()
                .unwrap_or_else(|e| e.into_inner()),
        );
        for m in messages {
            let Some(s) = self.studio.as_mut().filter(|s| {
                s.canvas.session == m.session
                    && s.screen.is_none()
                    && !s.design.manifest.system_workspace
            }) else {
                continue;
            };
            if s.canvas.seen.contains(&m.request_id) {
                continue;
            }
            s.canvas.seen.push_back(m.request_id);
            if s.canvas.seen.len() > 512 {
                s.canvas.seen.pop_front();
            }
            if s.comment_mode && matches!(&m.action,
                Action::Open { .. } | Action::Resize { .. } | Action::Position { .. }
                | Action::MoveScreen { .. } | Action::ReorderSections { .. }
                | Action::ContextAction { .. }) {
                continue;
            }
            let known = |id: &Uuid| {
                s.design
                    .manifest
                    .screens
                    .iter()
                    .any(|screen| screen.id == *id && !screen.archived)
            };
            match m.action {
                Action::CommentToggle => self.studio_comment_mode(cx),
                Action::CommentFocus { .. } => {}, // Cross-screen navigation belongs to the standalone player.
                Action::CommentSend { id, revision } => {
                    if s.comment_mode { self.studio_send_comment(m.request_id, id, revision, false, cx); }
                }
                Action::CommentDraft { dirty } => {
                    if s.comment_mode { s.canvas.comment_draft = dirty; }
                }
                Action::CommentsRead => {
                    if s.comment_mode { self.studio_comments_request(m.request_id, None, false, cx); }
                }
                Action::CommentEdit { revision, operation } => {
                    if s.comment_mode { self.studio_comments_request(m.request_id, Some((revision, operation)), false, cx); }
                }
                Action::Ready => {
                    s.canvas.ready = true;
                    s.canvas.fingerprint.clear();
                    let fit_all=std::mem::take(&mut s.canvas.fit_all_requested);
                    self.refresh_studio_canvas(cx);
                    if self.studio.as_ref().is_some_and(|s|s.inline_screen.is_some()) {self.rebuild_studio_editor(cx);}
                    if fit_all {self.studio_canvas_command("fit-all",cx);}
                    self.studio_canvas_fit_pending(cx);
                }
                Action::Failed { error } => {
                    s.error = Some(format!("Canvas unavailable; showing Grid. {error}"));
                    s.comment_mode = false;
                    s.canvas.failed = true;
                    s.canvas.leave();
                }
                Action::Select { screen_id } => {
                    if screen_id.as_ref().is_none_or(known)
                        && (s.canvas.layout.selected_screen_id != screen_id
                            || s.canvas.layout.selected_section_id.is_some())
                    {
                        // Screen and section selection are exclusive.
                        s.canvas.layout.select_screen(screen_id);
                        if screen_id.is_none() { s.canvas.layout.select_section(None); }
                        if let Err(e) = s.canvas.save(&s.store, s.design.manifest.id) {
                            s.error = Some(e.to_string());
                        }
                        s.section_name_input = None;
                        self.studio_canvas_selection(cx);
                    }
                }
                Action::SelectSection { section_id } => {
                    if section_id.is_none_or(|id| s.design.manifest.section(id).is_some()) {
                        self.studio_select_section(section_id, cx);
                    }
                }
                Action::MoveScreen { screen_id, section_id, before_screen_id, position, revision, fingerprint } => {
                    let request = Some((m.session, m.request_id));
                    if !known(&screen_id) || section_id.is_some_and(|id| s.design.manifest.section(id).is_none()) {
                        self.studio_canvas_move_result(request, Some("The screen or section no longer exists.".into()), cx);
                        continue;
                    }
                    let grouped = s.design.manifest.section_of(screen_id).is_some();
                    if section_id.is_none() && !grouped && before_screen_id.is_none() {
                        // Free screens moving on the canvas are personal layout only.
                        if let Some(position) = position { s.canvas.layout.positions.insert(screen_id, position); }
                        if let Err(e) = s.canvas.save(&s.store, s.design.manifest.id) { s.error = Some(e.to_string()); }
                        self.studio_canvas_move_result(request, None, cx);
                        continue;
                    }
                    let effect = super::studio_sections::GroupingEffect {
                        unsectioned: if section_id.is_none() && position.is_none() { vec![screen_id] } else { vec![] },
                        drop: section_id.is_none().then_some(position).flatten().map(|p| (screen_id, p)),
                        canvas_request: request,
                        ..Default::default()
                    };
                    let operation = StudioOperation::MoveScreenToSection { screen_id, section_id, before_screen_id };
                    self.studio_group(vec![operation], effect, Some((revision, fingerprint)), cx);
                }
                Action::ReorderSections { section_ids, revision, fingerprint } => {
                    let effect = super::studio_sections::GroupingEffect { canvas_request: Some((m.session, m.request_id)), ..Default::default() };
                    self.studio_group(vec![StudioOperation::ReorderSections { section_ids }], effect, Some((revision, fingerprint)), cx);
                }
                Action::ContextAction { screen_id, section_id, action } => {
                    if let Some(id) = screen_id.filter(|id| s.design.manifest.screens.iter().any(|p| p.id == *id)) {
                        let name = s.design.manifest.screens.iter().find(|p| p.id == id).map(|p| p.name.clone()).unwrap_or_default();
                        match action.as_str() {
                            "rename" => self.studio_name_dialog("Rename screen", &name, super::studio::NameAction::Screen(Some(id)), window, cx),
                            "new-section" => self.studio_new_section_with(id, window, cx),
                            action => self.studio_screen_menu_action(id, action, cx),
                        }
                    } else if let Some(id) = section_id {
                        self.studio_section_menu_action(id, &action, window, cx);
                    }
                }
                Action::Open { screen_id } => {
                    if !s.comment_mode && known(&screen_id) {
                        s.canvas.focus_screen = None;
                        self.studio_inline_select(Some(screen_id), cx);
                    }
                }
                Action::EditorFailed { screen_id, error } => {
                    if s.inline_screen==Some(screen_id) && !s.dirty && !s.saving {
                        s.error=Some(error);
                        self.studio_finish_inline(None,cx);
                    }
                }
                Action::CloseEditor => {
                    s.canvas.focus_screen = None;
                    self.studio_inline_select(None,cx);
                },
                Action::Camera { viewport } => {
                    if s.canvas.layout.viewport == viewport {
                        continue;
                    }
                    s.canvas.layout.viewport = viewport;
                    if let Err(e) = s.canvas.save(&s.store, s.design.manifest.id) {
                        s.error = Some(e.to_string());
                    }
                }
                Action::Position {
                    screen_id,
                    position,
                } => {
                    // Grouped screens are positioned by their section, never freely.
                    if known(&screen_id)
                        && s.design.manifest.section_of(screen_id).is_none()
                        && s.canvas.layout.positions.get(&screen_id) != Some(&position)
                    {
                        s.canvas.layout.positions.insert(screen_id, position);
                        if let Err(e) = s.canvas.save(&s.store, s.design.manifest.id) {
                            s.error = Some(e.to_string());
                        }
                    }
                }
                Action::Previews { requests } => {
                    if visible {
                        s.canvas.desired = requests
                            .into_iter()
                            .filter(|r| {
                                known(&r.screen_id)
                                    && s.canvas.keys.get(&r.screen_id) == Some(&r.content_key)
                            })
                            .collect();
                        let desired = &s.canvas.desired;
                        s.canvas.delivered.retain(|r| desired.contains(r));
                    }
                }
                Action::Resize {
                    screen_id,
                    width,
                    height,
                    x,
                    y,
                    revision,
                    fingerprint,
                } => {
                    if s.inline_screen==Some(screen_id) {continue;}
                    self.studio_canvas_resize(
                        m.request_id,
                        screen_id,
                        width,
                        height,
                        StudioCanvasPoint { x, y },
                        revision,
                        fingerprint,
                        cx,
                    );
                }
                Action::Flushed => {
                    if s.canvas.flush_request != Some(m.request_id) {
                        continue;
                    }
                    s.canvas.flush_request = None;
                    if let Err(e) = s.canvas.save(&s.store, s.design.manifest.id) {
                        s.error = Some(format!("Could not save canvas layout: {e}"));
                        s.pending_navigation = None;
                        continue;
                    }
                    if s.saving {
                        s.canvas.navigation_ready = true;
                        continue;
                    }
                    let action = s.pending_navigation.take();
                    s.inline_screen=None;s.inline_flush=None;s.editor_html=None;s.dirty=false;
                    s.canvas.leave();
                    if let Some(action) = action {
                        action(self, cx);
                    }
                }
            }
        }
        if let Some(s) = self.studio.as_mut() {
            if !visible {
                s.canvas.desired.clear();
                s.canvas.delivered.clear();
            }
            if visible
                && s.canvas.active()
                && s.screen.is_none()
                && !s.canvas.ready
                && s.canvas.opened.elapsed() > std::time::Duration::from_secs(10)
            {
                s.canvas.failed = true;
                s.canvas.leave();
                s.error =
                    Some("Canvas did not initialize; showing Grid. Select Canvas to retry.".into());
            }
        }
        if visible {
            self.queue_studio_canvas(cx);
        }
    }
    fn queue_studio_canvas(&mut self, cx: &mut Context<Self>) {
        if !self.studio_canvas_active() || !self.studio_workspace_visible(cx) {
            return;
        }
        let Some(s) = self.studio.as_mut() else {
            return;
        };
        if !s.canvas.ready || s.canvas.busy || std::time::Instant::now() < s.canvas.retry_at {
            return;
        }
        while let Some(request) = s
            .canvas
            .desired
            .iter()
            .find(|r| !s.canvas.delivered.contains(*r))
            .cloned()
        {
            if let Some(value) = cached(s.canvas.session, &request) {
                s.canvas.delivered.insert(request);
                self.web_host
                    .update(cx, |host, _| host.canvas_reply(&value));
                continue;
            }
            s.canvas.busy = true;
            let worker = Uuid::new_v4();
            s.canvas.worker = Some(worker);
            let store = s.store.clone();
            let design = StudioDesign {
                manifest: s.design.manifest.clone(),
                documents: s
                    .design
                    .documents
                    .iter()
                    .filter(|(id, _)| **id == request.screen_id)
                    .map(|(id, doc)| (*id, doc.clone()))
                    .collect(),
                system: s.design.system.clone(),
                overrides: s.design.overrides.clone(),
                fingerprint: s.design.fingerprint.clone(),
                asset_fingerprint: s.design.asset_fingerprint.clone(),
            };
            let session = s.canvas.session;
            cx.spawn(async move |this,cx|{
                let job=request.clone();let result=cx.background_executor().spawn(async move{super::studio_editor::canvas_preview(&store,&design,job.screen_id,job.tier)}).await;
                let _=this.update(cx,|this,cx|{
                    let Some(s)=this.studio.as_mut().filter(|s|s.canvas.worker==Some(worker)) else{return;};s.canvas.busy=false;s.canvas.worker=None;
                    // A completed image remains useful when the camera comes back.
                    // Cache it even if demand moved on; only current content may
                    // be delivered, and a coarse result cannot complete a sharp job.
                    let wanted=s.canvas.session==session && s.canvas.desired.iter().any(|r|
                        r.screen_id==request.screen_id && r.content_key==request.content_key && r.tier>=request.tier);
                    let value=match result {
                        Ok(Some(bytes))=>match insert(&request,bytes){Ok(())=>if wanted {cached(session,&request)} else {None},Err(e)=>Some(json!({"session":session,"type":"preview-failed","screen_id":request.screen_id,"content_key":request.content_key,"error":e.to_string()}))},
                        Ok(None)=>{s.canvas.retry_at=std::time::Instant::now()+std::time::Duration::from_millis(250);None},
                        Err(e)=>Some(json!({"session":session,"type":"preview-failed","screen_id":request.screen_id,"content_key":request.content_key,"error":e.to_string()})),
                    };
                    if wanted {if let Some(value)=value{s.canvas.delivered.insert(request);this.web_host.update(cx,|host,_|host.canvas_reply(&value));}}
                    this.queue_studio_canvas(cx);
                    // The WebView consumes this image reply directly. No GPUI
                    // presentation changed, so keep chat/history caches intact.
                });
            }).detach();
            break;
        }
    }
    fn studio_canvas_resize(
        &mut self,
        request: Uuid,
        id: Uuid,
        width: u32,
        height: u32,
        position: StudioCanvasPoint,
        revision: u64,
        fingerprint: String,
        cx: &mut Context<Self>,
    ) {
        let Some(s) = self.studio.as_mut() else {
            return;
        };
        let Some(mut screen) = s
            .design
            .manifest
            .screens
            .iter()
            .find(|screen| screen.id == id && !screen.archived)
            .cloned()
        else {
            let reply = resize_result(s, request, id, Some("This screen was archived or removed while resizing.".into()));
            self.web_host
                .update(cx, |host, _| host.canvas_reply(&reply));
            return;
        };
        if s.saving || s.grouping.is_some() || revision != s.design.manifest.revision || fingerprint != s.design.fingerprint
        {
            let reply = resize_result(s, request, id, Some("Screen changed while resizing. Try again after the current edit finishes.".into()));
            self.web_host
                .update(cx, |host, _| host.canvas_reply(&reply));
            return;
        }
        screen.width = width;
        screen.height = height;
        let scope = StudioTurnScope::whole_design(&s.design);
        let tx = StudioTransaction {
            id: request,
            scope_id: scope.id,
            design_id: scope.design_id,
            expected_revision: revision,
            expected_fingerprint: fingerprint,
            operations: vec![StudioOperation::UpdateScreen { screen }],
        };
        let store = s.store.clone();
        let session = s.canvas.session;
        let design_id = s.design.manifest.id;
        s.saving = true;
        cx.spawn(async move|this,cx|{
            let result=cx.background_executor().spawn(async move{let result=store.apply(&scope,&tx);let latest=store.load(design_id);(result,latest)}).await;
            let _=this.update(cx,|this,cx|{
                let Some(s)=this.studio.as_mut().filter(|s|s.design.manifest.id==design_id) else{return;};s.saving=false;
                // A grouped screen's section reflows around its new size instead.
                let error=match result.0{Ok(design)=>{s.design=design;s.redo=false;if s.design.manifest.section_of(id).is_none(){s.canvas.layout.positions.insert(id,position);}s.canvas.save(&s.store,design_id).err().map(|e|e.to_string())},Err(e)=>{if let Ok(design)=result.1{s.design=design;}Some(e.to_string())}};
                if s.canvas.session==session{let reply=resize_result(s,request,id,error.clone());this.web_host.update(cx,|host,_|host.canvas_reply(&reply));}
                if let Some(error)=error{s.error=Some(error);}
                let navigation=if s.canvas.navigation_ready{s.canvas.navigation_ready=false;s.inline_screen=None;s.inline_flush=None;s.editor_html=None;s.dirty=false;s.canvas.leave();s.pending_navigation.take()}else{None};
                this.refresh_studio_canvas(cx);if let Some(action)=navigation{action(this,cx);}cx.notify();
            });
        }).detach();
    }
}

impl CenterArea {
    /// Disk access and the project lock stay off the render thread. Comment
    /// results do not invalidate design metadata or demand new preview images.
    fn studio_comments_request(&mut self, request: Uuid, edit: Option<(u64, StudioCommentOperation)>, editor: bool, cx: &mut Context<Self>) {
        let Some(s) = self.studio.as_mut() else { return; };
        if s.canvas.comments_busy {
            self.studio_comment_result(request, editor, None, Some("A comment request is still running. Try again.".into()), false, cx);
            return;
        }
        let owner = s.canvas.begin_comment_request();
        let store = s.store.clone();
        let design_id = s.design.manifest.id;
        let session = if editor { s.editor_session } else { s.canvas.session };
        cx.spawn(async move |this, cx| {
            let (comments, error) = cx.background_executor().spawn(async move {
                match edit {
                    Some((revision, operation)) => match store.apply_comment(design_id, revision, &operation) {
                        Ok(comments) => (Some(comments), None),
                        Err(error) => (store.comments(design_id).ok(), Some(error.to_string())),
                    },
                    None => match store.comments(design_id) {
                        Ok(comments) => (Some(comments), None),
                        Err(error) => (None, Some(error.to_string())),
                    },
                }
            }).await;
            let _ = this.update(cx, |this, cx| {
                let Some(s) = this.studio.as_mut().filter(|s| s.design.manifest.id == design_id) else { return; };
                if !s.canvas.finish_comment_request(owner) { return; }
                // The operation owns the busy state even if its screen was rebuilt.
                // Release it before dropping the reply for the old web session.
                cx.notify();
                if (if editor { s.editor_session } else { s.canvas.session }) != session { return; }
                this.studio_comment_result(request, editor, comments, error, false, cx);
                cx.notify();
            });
        }).detach();
    }
    fn studio_comment_result(&self, request: Uuid, editor: bool, comments: Option<StudioComments>, error: Option<String>, sent: bool, cx: &App) {
        let Some(s) = self.studio.as_ref() else { return; };
        let session = if editor { s.editor_session } else { s.canvas.session };
        let reply = json!({"session":session,"type":"comments-result","request_id":request,"comments":comments,"error":error,"sent":sent});
        if editor { self.web_host.read(cx).studio_reply(&reply); }
        else { self.web_host.read(cx).canvas_reply(&reply); }
    }
    pub(super) fn studio_comment_action(&mut self, message: Message, editor: bool, cx: &mut Context<Self>) {
        let Some(s) = self.studio.as_mut() else { return; };
        if !s.comment_mode || message.request_id.is_nil() || message.session != if editor { s.editor_session } else { s.canvas.session } { return; }
        if s.canvas.seen.contains(&message.request_id) { return; }
        s.canvas.seen.push_back(message.request_id);
        if s.canvas.seen.len() > 512 { s.canvas.seen.pop_front(); }
        match message.action {
            Action::CommentsRead => self.studio_comments_request(message.request_id, None, editor, cx),
            Action::CommentDraft { dirty } => s.canvas.comment_draft = dirty,
            Action::CommentEdit { revision, operation } if operation.validate().is_ok() => self.studio_comments_request(message.request_id, Some((revision, operation)), editor, cx),
            Action::CommentSend { id, revision } if !id.is_nil() => self.studio_send_comment(message.request_id, id, revision, editor, cx),
            Action::CommentFocus { id } if !id.is_nil() => self.studio_focus_comment(message.request_id, id, editor, cx),
            _ => {}
        }
    }
    fn studio_focus_comment(&mut self, request: Uuid, id: Uuid, editor: bool, cx: &mut Context<Self>) {
        let Some(s) = self.studio.as_mut() else { return; };
        if !editor { return; }
        if s.canvas.comments_busy || s.canvas.comment_draft || s.dirty || s.saving {
            self.studio_comment_result(request, true, None, Some("Post or cancel your draft and wait for pending saves before opening another screen.".into()), false, cx);
            return;
        }
        let owner = s.canvas.begin_comment_request();
        let session = s.editor_session;
        let design = s.design.manifest.clone();
        let design_id = design.id;
        let store = s.store.clone();
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move {
                let comments = store.comments(design.id)?;
                let pin = comments.pins.into_iter().find(|pin| pin.id == id && !pin.resolved)
                    .context("This comment is no longer open")?;
                anyhow::ensure!(design.screens.iter().any(|screen| screen.id == pin.screen_id && !screen.archived), "This screen is no longer available");
                Ok::<_, anyhow::Error>(pin)
            }).await;
            let _ = this.update(cx, |this, cx| {
                let Some(s) = this.studio.as_mut().filter(|s| s.design.manifest.id == design_id) else { return; };
                if !s.canvas.finish_comment_request(owner) { return; }
                cx.notify();
                if s.editor_session != session || !s.comment_mode { return; }
                match result {
                    Ok(pin) => {
                        if s.dirty || s.saving || s.canvas.comment_draft {
                            this.studio_comment_result(request, true, None, Some("Wait for the screen to finish saving before opening another comment.".into()), false, cx);
                            return;
                        }
                        if !s.design.manifest.screens.iter().any(|screen| screen.id == pin.screen_id && !screen.archived) {
                            this.studio_comment_result(request, true, None, Some("This screen is no longer available".into()), false, cx);
                            return;
                        }
                        s.comment_selected = Some(pin.id);
                        if s.prototype && s.screen != Some(pin.screen_id) {
                            if let Some(previous) = s.screen { s.prototype_history.push(previous); }
                        }
                        if !s.prototype && s.screen.is_some() {
                            s.screen = Some(pin.screen_id);
                            s.viewing_size = None;
                            this.rebuild_studio_editor(cx);
                        } else {
                            this.studio_select_screen(Some(pin.screen_id), cx);
                        }
                    }
                    Err(error) => this.studio_comment_result(request, true, None, Some(format!("{error:#}")), false, cx),
                }
                cx.notify();
            });
        }).detach();
    }
    fn studio_send_comment(&mut self, request: Uuid, id: Uuid, revision: u64, editor: bool, cx: &mut Context<Self>) {
        let Some(s) = self.studio.as_mut() else { return; };
        if s.canvas.comments_busy || s.dirty || s.saving {
            self.studio_comment_result(request, editor, None, Some("Wait for the current save or comment request to finish before sending.".into()), false, cx); return;
        }
        let owner = s.canvas.begin_comment_request();
        let store = s.store.clone(); let design_id = s.design.manifest.id;
        let session = if editor { s.editor_session } else { s.canvas.session };
        cx.spawn(async move |this, cx| {
            // Read the persisted note, never client-supplied text or screen IDs.
            let result = cx.background_executor().spawn(async move {
                let comments = store.comments(design_id)?;
                anyhow::ensure!(comments.revision == revision, "Comments changed. Reload comments before sending.");
                let pin = comments.pins.iter().find(|pin| pin.id == id && !pin.resolved).cloned().ok_or_else(|| anyhow::anyhow!("This comment is no longer open."))?;
                Ok::<_, anyhow::Error>((comments, pin))
            }).await;
            let _ = this.update(cx, |this, cx| {
                let Some(s) = this.studio.as_mut().filter(|s| s.design.manifest.id == design_id) else { return; };
                if !s.canvas.finish_comment_request(owner) { return; }
                cx.notify();
                if (if editor { s.editor_session } else { s.canvas.session }) != session || !s.comment_mode { return; }
                let (comments, pin) = match result {
                    Ok(value) => value,
                    Err(error) => { this.studio_comment_result(request, editor, None, Some(error.to_string()), false, cx); return; }
                };
                let Some(screen) = s.design.manifest.screens.iter().find(|screen| screen.id == pin.screen_id && !screen.archived) else {
                    this.studio_comment_result(request, editor, Some(comments), Some("This screen is no longer available.".into()), false, cx); return;
                };
                if s.dirty || s.saving {
                    this.studio_comment_result(request, editor, Some(comments), Some("Wait for the screen to finish saving before sending.".into()), false, cx); return;
                }
                let screen_name = screen.name.clone();
                let project = s.project; let root = s.store.project.clone(); let chat = s.relative_chat.clone(); let cache = s.store.cache.clone();
                let record = this.doc_assistants.update(cx, |assistants, cx| assistants.ensure_record(project, chat, cx));
                this.hydrate_doc_assistant_chat_session(&record, &root, cx);
                let agent = Self::doc_assistant_agent_record(&record, root.clone());
                let Some(context) = agent.studio_context.clone() else {
                    this.studio_comment_result(request, editor, Some(comments), Some("Open a design assistant conversation before sending.".into()), false, cx); return;
                };
                let target = comment_agent_request(context, root, cache, &pin, screen_name.clone(), this.workspace.read(cx).studio.request_guidance());
                let prompt = comment_agent_prompt(&pin, &screen_name);
                let accepted = this.dispatch_agent_chat_submission_with_studio_request(agent.id, prompt, None, Vec::new(), AgentInteractionMode::Default, Some(agent), false, Some(target), cx);
                if accepted {
                    if let Some(s) = this.studio.as_mut() { s.tab = super::studio::StudioTab::Agent; s.sidebar_collapsed = false; }
                }
                let error = (!accepted).then(|| this.agent_start_errors.get(&record.chat_agent_id).cloned().unwrap_or_else(|| "Could not send to the design assistant. Check its provider setup and try again.".into()));
                this.studio_comment_result(request, editor, Some(comments), error, accepted, cx);
                cx.notify();
            });
        }).detach();
    }
}

fn comment_agent_prompt(pin: &StudioCommentPin, screen: &str) -> String {
    format!("Fix the feedback in this Studio comment on screen {screen:?} (screen ID {}, comment ID {}).\nPinned location: {:.1}% from the left, {:.1}% from the top.\n\n{}\n\nApply the fix to this screen using the existing design system. Leave the comment open for my review.", pin.screen_id, pin.id, pin.x * 100., pin.y * 100., pin.body)
}

fn comment_agent_request(context: StudioAgentContext, project: std::path::PathBuf, cache: std::path::PathBuf, pin: &StudioCommentPin, target_label: String, design_guidance: String) -> crate::state::agent_chat::StudioChatRequest {
    crate::state::agent_chat::StudioChatRequest { context, project, cache, current_screen_id: Some(pin.screen_id), current_section_id: None, selected_element: None, design_guidance, target_label }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn comment_runtime() -> Runtime {
        let root = std::env::temp_dir().join(format!("choro-comment-runtime-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("project")).unwrap();
        let store = StudioStore::new(root.join("project"), root.join("data")).unwrap();
        let design = store.create("Comment request lifecycle").unwrap();
        Runtime::new(&store, &design).0
    }
    #[test]
    fn stale_comment_completion_releases_its_lock_without_unlocking_a_new_request() {
        let mut runtime = comment_runtime();
        let owner = runtime.begin_comment_request();
        // A refreshed web surface has a new session, but the original operation
        // still owns the workspace's busy state until its completion is handled.
        runtime.session = Uuid::new_v4();
        assert!(runtime.finish_comment_request(owner));
        assert!(!runtime.comments_busy);
        let stale = runtime.begin_comment_request();
        runtime.leave();
        let current = runtime.begin_comment_request();
        assert!(!runtime.finish_comment_request(stale));
        assert!(runtime.comments_busy);
        assert!(runtime.finish_comment_request(current));
        assert!(!runtime.comments_busy);
    }
    #[test]
    fn comment_refresh_protection_lasts_until_posting_or_cancelling_and_requests_finish() {
        let mut runtime = comment_runtime();
        assert!(!runtime.comment_surface_pending());
        runtime.comment_draft = true;
        assert!(runtime.comment_surface_pending());
        let post = runtime.begin_comment_request();
        runtime.comment_draft = false;
        assert!(runtime.comment_surface_pending(), "Clearing draft text must not allow refresh during its save");
        assert!(runtime.finish_comment_request(post));
        assert!(!runtime.comment_surface_pending(), "The next poll can apply the latest screen after posting");
        runtime.comment_draft = true;
        runtime.comment_draft = false;
        assert!(!runtime.comment_surface_pending(), "Cancelling also allows the next refresh");
    }
    #[test]
    fn comment_navigation_uses_only_a_saved_pin_in_the_current_session() {
        let session = Uuid::new_v4();
        let mut value = json!({"session":session,"request_id":Uuid::new_v4(),"type":"comment-focus","id":Uuid::new_v4()});
        assert!(enqueue(&value.to_string(), session));
        assert!(!enqueue(&value.to_string(), Uuid::new_v4()));
        value["screen_id"] = json!(Uuid::new_v4());
        assert!(!enqueue(&value.to_string(), session));
        value.as_object_mut().unwrap().remove("screen_id");
        value["body"] = json!("Forged note");
        assert!(!enqueue(&value.to_string(), session));
        value.as_object_mut().unwrap().remove("body");
        value["id"] = json!(Uuid::nil());
        assert!(!enqueue(&value.to_string(), session));
    }
    #[test]
    fn comment_handoff_rejects_client_text_and_foreign_sessions() {
        let session = Uuid::new_v4();
        let mut value = json!({"session":session,"request_id":Uuid::new_v4(),"type":"comment-send","revision":1,"id":Uuid::new_v4()});
        assert!(enqueue(&value.to_string(), session));
        assert!(!enqueue(&value.to_string(), Uuid::new_v4()));
        value["body"] = json!("Forged feedback");
        assert!(!enqueue(&value.to_string(), session));
        value.as_object_mut().unwrap().remove("body");
        value["id"] = json!(Uuid::nil());
        assert!(!enqueue(&value.to_string(), session));
    }
    #[test]
    fn comment_handoff_activates_the_pinned_screen_without_an_element_target() {
        let root = std::env::temp_dir().join(format!("choro-comment-handoff-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("project")).unwrap();
        let store = StudioStore::new(root.join("project"), root.join("data")).unwrap();
        let design = store.create("Comment handoff").unwrap();
        let pin = StudioCommentPin { id: Uuid::new_v4(), screen_id: design.manifest.screens[0].id, x: 0.25, y: 0.75, body: "Fix the action".into(), resolved: false, created_at: 1 };
        let context = StudioAgentContext { target: StudioAgentTarget::Design, design_id: design.manifest.id, conversation_id: store.conversations(design.manifest.id).unwrap().selected };
        let request = comment_agent_request(context, store.project.clone(), store.cache.clone(), &pin, "Pinned screen".into(), "Preserve the theme".into());
        let agent = Uuid::new_v4();
        request.activate(agent, &comment_agent_prompt(&pin, "Pinned screen")).unwrap();
        let scope = store.scope(agent).unwrap();
        assert_eq!(scope.current_screen_id, Some(pin.screen_id));
        assert_eq!(scope.selected_element, None);
        assert_eq!(scope.design_guidance, "Preserve the theme");
        assert!(comment_agent_prompt(&pin, "Pinned screen").contains(&pin.body));
    }
    #[test]
    fn comments_bridge_limits_text_and_coordinates_and_has_no_reply_operation() {
        let session = Uuid::new_v4();
        let screen = Uuid::new_v4();
        let wrap = |operation: Value| json!({"session":session,"request_id":Uuid::new_v4(),"type":"comment-edit","revision":0,"operation":operation}).to_string();
        let create = json!({"operation":"create","id":Uuid::new_v4(),"screen_id":screen,"x":0.25,"y":0.75,"body":"Move this button"});
        assert!(enqueue(&wrap(create.clone()), session));
        for (key, value) in [("x", json!(-0.1)), ("y", json!(1.1)), ("body", json!(" ")), ("body", json!("x".repeat(MAX_COMMENT_BODY + 1)))] {
            let mut invalid = create.clone(); invalid[key] = value;
            assert!(!enqueue(&wrap(invalid), session));
        }
        assert!(enqueue(&wrap(json!({"operation":"resolve","id":Uuid::new_v4()})), session));
        assert!(!enqueue(&wrap(json!({"operation":"reply","id":Uuid::new_v4(),"body":"Extra"})), session));
        assert!(!enqueue(&wrap(create), Uuid::new_v4()));
    }
    #[test]
    fn bridge_rejects_forgery_editor_commands_and_unbounded_inputs() {
        let session = Uuid::new_v4();
        let id = Uuid::new_v4();
        let screen = Uuid::new_v4();
        let wrap = |action: Value| {
            let mut v = json!({"session":session,"request_id":id});
            v.as_object_mut()
                .unwrap()
                .extend(action.as_object().unwrap().clone());
            v.to_string()
        };
        assert!(enqueue(&wrap(json!({"type":"ready"})), session));
        assert!(!enqueue(&wrap(json!({"type":"ready"})), Uuid::new_v4()));
        assert!(!enqueue(
            &wrap(json!({"type":"save","document":"forged"})),
            session
        ));
        assert!(!enqueue(
            &wrap(json!({"type":"position","screen_id":"../../private","position":{"x":0,"y":0}})),
            session
        ));
        assert!(!enqueue(
            &wrap(
                json!({"type":"resize","screen_id":screen,"width":99999,"height":960,"x":0,"y":0,"revision":1,"fingerprint":"a"})
            ),
            session
        ));
        assert!(!enqueue(
            &wrap(json!({"type":"camera","viewport":{"x":0,"y":0,"zoom":3}})),
            session
        ));
        assert!(!enqueue(&"x".repeat(MAX_MESSAGE + 1), session));
        assert!(enqueue(
            &wrap(
                json!({"type":"resize","screen_id":screen,"width":240,"height":4096,"x":-400,"y":0,"revision":1,"fingerprint":"a"})
            ),
            session
        ));
    }
    #[test]
    fn section_messages_are_validated_before_queueing() {
        let session = Uuid::new_v4();
        let (screen, section) = (Uuid::new_v4(), Uuid::new_v4());
        let wrap = |action: Value| {
            let mut v = json!({"session":session,"request_id":Uuid::new_v4()});
            v.as_object_mut().unwrap().extend(action.as_object().unwrap().clone());
            v.to_string()
        };
        assert!(enqueue(&wrap(json!({"type":"select-section","section_id":section})), session));
        assert!(enqueue(&wrap(json!({"type":"select-section","section_id":null})), session));
        let moving = json!({"type":"move-screen","screen_id":screen,"section_id":section,"before_screen_id":null,"position":null,"revision":3,"fingerprint":"abc"});
        assert!(enqueue(&wrap(moving.clone()), session));
        // Drag-out positions are bounded like every canvas coordinate.
        let mut out = moving.clone();
        out["section_id"] = Value::Null;
        out["position"] = json!({"x":1e9,"y":0});
        assert!(!enqueue(&wrap(out), session));
        let mut itself = moving.clone();
        itself["before_screen_id"] = json!(screen);
        assert!(!enqueue(&wrap(itself), session), "a screen cannot precede itself");
        let mut forged = moving;
        forged["operations"] = json!([]);
        assert!(!enqueue(&wrap(forged), session), "unknown fields are rejected");
        let order = (0..=MAX_SECTIONS).map(|_| Uuid::new_v4()).collect::<Vec<_>>();
        assert!(!enqueue(&wrap(json!({"type":"reorder-sections","section_ids":order,"revision":1,"fingerprint":"a"})), session));
        assert!(enqueue(&wrap(json!({"type":"context-action","screen_id":screen,"section_id":null,"action":"duplicate"})), session));
        assert!(enqueue(&wrap(json!({"type":"context-action","screen_id":null,"section_id":section,"action":"ungroup"})), session));
        for (screen_id, section_id, action) in [
            (json!(screen), json!(section), "rename"),
            (Value::Null, Value::Null, "rename"),
            (json!(screen), Value::Null, "ungroup"),
            (Value::Null, json!(section), "write-screen"),
        ] {
            assert!(!enqueue(&wrap(json!({"type":"context-action","screen_id":screen_id,"section_id":section_id,"action":action})), session));
        }
        MESSAGES.get_or_init(Default::default).lock().unwrap().clear();
    }
    #[test]
    fn image_keys_are_session_scoped_and_encoded_cache_is_bounded() {
        fn png(size: usize) -> Vec<u8> {
            let mut b = vec![0; size];
            b[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
            b[16..20].copy_from_slice(&256u32.to_be_bytes());
            b[20..24].copy_from_slice(&256u32.to_be_bytes());
            b
        }
        let session = Uuid::new_v4();
        let request = PreviewRequest {
            screen_id: Uuid::new_v4(),
            content_key: Uuid::new_v4().to_string(),
            tier: 256,
        };
        insert(&request, png(1024)).unwrap();
        let reply = cached(session, &request).unwrap();
        let key: Uuid = serde_json::from_value(reply["key"].clone()).unwrap();
        assert!(image_bytes(session, key).is_some());
        assert!(image_bytes(Uuid::new_v4(), key).is_none());
        revoke(session);
        assert!(image_bytes(session, key).is_none());
        for _ in 0..3 {
            let r = PreviewRequest {
                content_key: Uuid::new_v4().to_string(),
                ..request.clone()
            };
            insert(&r, png(12 * 1024 * 1024)).unwrap();
        }
        assert!(images().bytes <= IMAGE_BUDGET);
    }
}
