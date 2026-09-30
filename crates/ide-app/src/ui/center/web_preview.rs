//! Single native web surface shared by design previews and chat visualizations.
//!
//! GPUI paints the application into one GPU surface. `WKWebView` is therefore a
//! native child layered above a rectangle reserved by GPUI. Keeping exactly one
//! live child avoids focus, clipping, and z-order problems when chat rows scroll
//! or dialogs open.

use std::path::PathBuf;

use gpui::App;

/// Wake message handling directly without refreshing every cached view in the app.
#[derive(Clone)]
pub(crate) struct WebPreviewWake {
    sender: async_channel::Sender<()>,
    general: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl WebPreviewWake {
    pub(super) fn new(sender: async_channel::Sender<()>) -> Self {
        Self { sender, general: Default::default() }
    }
    pub(super) fn take_general(&self) -> bool { self.general.swap(false, std::sync::atomic::Ordering::AcqRel) }
    fn refresh(&self) -> Result<(), async_channel::TrySendError<()>> {
        self.general.store(true, std::sync::atomic::Ordering::Release);
        self.sender.try_send(())
    }
    fn studio_refresh(&self) -> Result<(), async_channel::TrySendError<()>> { self.sender.try_send(()) }
}
#[cfg(test)]
mod wake_tests {
    use super::WebPreviewWake;
    #[test]
    fn coalescing_a_studio_pulse_preserves_a_general_message_wake() {
        let (sender, receiver) = async_channel::bounded(1);
        let wake = WebPreviewWake::new(sender);
        wake.studio_refresh().unwrap();
        assert!(wake.refresh().is_err(), "the bounded pulse is already queued");
        assert!(wake.take_general(), "the general message flag survives coalescing");
        assert!(!wake.take_general());
        receiver.try_recv().unwrap();
        assert!(!wake.take_general(), "a consumed flag cannot cause idle redraws");
    }
}
use gpui_component::ActiveTheme;
use ide_core::project::ProjectId;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::state::docs::ChoroDocument;

const MAX_VISUALIZATION_BYTES: u64 = 2 * 1024 * 1024;

/// Cached transcripts reuse a native placement. Only a real transcript layout
/// can prove that its inline visualization has left the visible list.
#[derive(Default)]
struct InlinePlacement {
    laying_out: bool,
    placed: bool,
    hidden: bool,
}
impl InlinePlacement {
    fn begin(&mut self) { self.laying_out = true; self.placed = false; }
    fn place(&mut self) { self.placed = true; self.hidden = false; }
    fn finish(&mut self) {
        if std::mem::take(&mut self.laying_out) { self.hidden = !self.placed; }
    }
}

#[cfg(test)]
mod inline_placement_tests {
    use super::InlinePlacement;
    #[test]
    fn cached_frames_preserve_visibility_and_scrolling_out_hides_the_surface() {
        let mut placement = InlinePlacement::default();
        placement.begin(); placement.place(); placement.finish();
        for _ in 0..100 { placement.finish(); assert!(!placement.hidden); }
        placement.begin(); placement.finish();
        assert!(placement.hidden, "a layout without the row must hide its native surface");
        placement.finish(); assert!(placement.hidden);
        placement.begin(); placement.place(); placement.finish();
        assert!(!placement.hidden, "scrolling back must restore the surface");
    }
}

/// Popup lifetimes can overlap (including a menu opening a dialog).
#[derive(Default)]
struct NativeOverlays {
    popups: usize,
    modal: bool,
}

impl NativeOverlays {
    fn hidden(&self) -> bool {
        self.modal || self.popups > 0
    }

    fn begin_popup(&mut self) {
        self.popups += 1;
    }

    fn end_popup(&mut self) {
        self.popups = self.popups.saturating_sub(1);
    }
}

/// Keep the editing document alive while GPUI paints a menu over its native view.
pub(super) fn suspend_for_menu(
    host: gpui::Entity<WebPreviewHost>,
    cx: &mut gpui::Context<gpui_component::menu::PopupMenu>,
) {
    host.update(cx, |host, _| host.begin_popup());
    cx.on_release(move |_, cx| {
        host.update(cx, |host, _| host.end_popup());
    })
    .detach();
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WebPreviewIntent {
    Url(String),
    StudioCanvas { session:Uuid, document:String },
    Studio {
        session: Uuid,
        document: String,
    },
    ProjectPreview {
        project_id: ProjectId,
        url: String,
        revision: u64,
    },
    DocEditor {
        path: PathBuf,
        document: ChoroDocument,
        files: Vec<DocEditorMention>,
        assets: Vec<DocEditorMention>,
        theme: DocEditorTheme,
    },
    Visualization {
        key: String,
        path: PathBuf,
        theme: VisualizationTheme,
    },
}

#[derive(Clone, Debug)]
pub enum ProjectPreviewMessage {
    ConsoleEntry {
        project_id: ProjectId,
        entry: ProjectPreviewConsoleEntry,
    },
    ToggleFocusMode {
        project_id: ProjectId,
    },
    Cancelled {
        project_id: ProjectId,
    },
    SelectionReady {
        project_id: ProjectId,
        target_kind: String,
    },
    SubmitReview {
        project_id: ProjectId,
        comment: String,
        target_kind: String,
        element: Option<ide_core::visual_review::VisualElementSelection>,
        area: Option<ide_core::visual_review::VisualAreaSelection>,
    },
    CaptureReady {
        project_id: ProjectId,
        agent_id: Uuid,
        url: String,
        comment: String,
        target_kind: String,
        element: Option<ide_core::visual_review::VisualElementSelection>,
        area: Option<ide_core::visual_review::VisualAreaSelection>,
        image_base64: String,
    },
    CaptureFailed {
        project_id: ProjectId,
        message: String,
    },
    AgentActionResult {
        project_id: ProjectId,
        command_id: Uuid,
        success: bool,
        capture: bool,
        result_json: String,
        error: Option<String>,
    },
    AgentNavigationReady {
        project_id: ProjectId,
        command_id: Uuid,
        result_json: String,
    },
    AgentNavigationSettled {
        project_id: ProjectId,
        command_id: Uuid,
        url: String,
    },
    AgentPageLoad {
        project_id: ProjectId,
        finished: bool,
        url: String,
    },
    AgentNativeInputRequest {
        project_id: ProjectId,
        command_id: Uuid,
        action: String,
        x: Option<f64>,
        y: Option<f64>,
        key: Option<String>,
        code: Option<String>,
        meta: bool,
        control: bool,
        alt: bool,
        shift: bool,
    },
    AgentSnapshotReady {
        project_id: ProjectId,
        command_id: Uuid,
        result_json: String,
        image_base64: String,
    },
    AgentSnapshotFailed {
        project_id: ProjectId,
        command_id: Uuid,
        message: String,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProjectPreviewConsoleLevel {
    Debug,
    Log,
    Info,
    Warn,
    Error,
}

impl ProjectPreviewConsoleLevel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Debug => "DEBUG",
            Self::Log => "LOG",
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
        }
    }

    pub fn is_error(self) -> bool {
        self == Self::Error
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPreviewConsoleEntry {
    pub level: ProjectPreviewConsoleLevel,
    pub message: String,
    pub source: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

/// The deliberately narrow set of messages accepted from page-adjacent
/// JavaScript. Screenshot completion is native-only and cannot be forged by a
/// preview page.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum ProjectPreviewInspectorMessage {
    ToggleFocusMode,
    Cancelled,
    SelectionReady {
        #[serde(rename = "targetKind")]
        target_kind: String,
    },
    SubmitReview {
        comment: String,
        #[serde(rename = "targetKind")]
        target_kind: String,
        element: Option<ide_core::visual_review::VisualElementSelection>,
        area: Option<ide_core::visual_review::VisualAreaSelection>,
    },
    AgentActionResult {
        #[serde(rename = "commandId")]
        command_id: Uuid,
        success: bool,
        capture: bool,
        result: Option<serde_json::Value>,
        error: Option<String>,
    },
    AgentNavigationReady {
        #[serde(rename = "commandId")]
        command_id: Uuid,
        result: serde_json::Value,
    },
    AgentNavigationSettled {
        #[serde(rename = "commandId")]
        command_id: Uuid,
        url: String,
    },
    AgentNativeInputRequest {
        #[serde(rename = "commandId")]
        command_id: Uuid,
        action: String,
        x: Option<f64>,
        y: Option<f64>,
        key: Option<String>,
        code: Option<String>,
        #[serde(default)]
        meta: bool,
        #[serde(default)]
        control: bool,
        #[serde(default)]
        alt: bool,
        #[serde(default)]
        shift: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocEditorMention {
    pub label: String,
    pub target: String,
    pub detail: String,
    pub kind: String,
    pub badge: String,
    pub preview_url: Option<String>,
    #[serde(skip)]
    pub preview_path: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DocEditorTheme {
    background: String,
    foreground: String,
    surface: String,
    muted: String,
    border: String,
    accent: String,
    danger: String,
    dark: bool,
}

impl DocEditorTheme {
    pub fn from_app(cx: &App) -> Self {
        Self {
            background: crate::ui::design::base(cx).to_string(),
            foreground: crate::ui::design::t1(cx).to_string(),
            surface: crate::ui::design::nav(cx).to_string(),
            muted: crate::ui::design::t3(cx).to_string(),
            border: crate::ui::design::line(cx).to_string(),
            accent: crate::ui::design::accent(cx).to_string(),
            danger: crate::ui::design::rose(cx).to_string(),
            dark: cx.theme().mode.is_dark(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum DocEditorMessage {
    Ready {
        path: String,
    },
    Change {
        path: PathBuf,
        document: ChoroDocument,
    },
    Upload {
        #[serde(rename = "requestId")]
        request_id: String,
        name: String,
        mime: String,
        #[serde(rename = "dataUrl")]
        data_url: String,
    },
    Error {
        message: String,
    },
    OpenReference {
        path: PathBuf,
        target: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisualizationTheme {
    background: String,
    foreground: String,
    card: String,
    card_foreground: String,
    muted: String,
    muted_foreground: String,
    border: String,
    primary: String,
    primary_foreground: String,
    accent: String,
    accent_foreground: String,
}

impl VisualizationTheme {
    pub fn from_app(cx: &App) -> Self {
        Self {
            background: crate::ui::design::base(cx).to_string(),
            foreground: crate::ui::design::t1(cx).to_string(),
            card: crate::ui::design::surface(cx).to_string(),
            card_foreground: crate::ui::design::t1(cx).to_string(),
            muted: crate::ui::design::surface_2(cx).to_string(),
            muted_foreground: crate::ui::design::t3(cx).to_string(),
            border: crate::ui::design::line(cx).to_string(),
            primary: crate::ui::design::focus(cx).to_string(),
            primary_foreground: crate::ui::design::t1(cx).to_string(),
            accent: crate::ui::design::accent_soft(cx).to_string(),
            accent_foreground: crate::ui::design::t1(cx).to_string(),
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::borrow::Cow;
    use std::cell::RefCell;
    use std::collections::{HashMap, VecDeque};
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::rc::Rc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use base64::Engine as _;
    use block2::RcBlock;
    use gpui::{Bounds, Pixels, Window};
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, MessageReceiver, NSObject, ProtocolObject};
    use objc2::{
        define_class, msg_send, sel, ClassType, DeclaredClass, MainThreadMarker, MainThreadOnly,
    };
    use objc2_app_kit::{
        NSBitmapImageFileType, NSBitmapImageRep,
        NSBitmapImageRepPropertyKey, NSEvent, NSEventMask, NSEventModifierFlags, NSEventType,
        NSImage, NSPasteboard, NSPasteboardWriting, NSView,
    };
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};
    use objc2_foundation::{
        NSArray, NSDictionary, NSError, NSObjectProtocol, NSPoint, NSProcessInfo, NSString, NSURL,
    };
    use objc2_web_kit::{
        WKContentWorld, WKScriptMessage, WKScriptMessageHandler, WKSnapshotConfiguration,
        WKUserContentController, WKUserScript, WKUserScriptInjectionTime,
    };
    use wry::dpi::{LogicalPosition, LogicalSize};
    use wry::http::{header::CONTENT_TYPE, Request, Response};
    use wry::{PageLoadEvent, Rect, WebView, WebViewBuilder, WebViewExtMacOS};

    use super::{
        WebPreviewWake, DocEditorMessage, ProjectId, ProjectPreviewConsoleEntry, ProjectPreviewInspectorMessage,
        ProjectPreviewMessage, Uuid, VisualizationTheme, WebPreviewIntent, MAX_VISUALIZATION_BYTES,
    };

    const MAX_DOC_UPLOAD_BYTES: usize = 25 * 1024 * 1024;
    const MAX_DOC_EDITOR_MESSAGE_BYTES: usize = 40 * 1024 * 1024;
    const DOC_EDITOR_JS: &[u8] = include_bytes!("../../../web/doc-editor/dist/editor.js");
    const DOC_EDITOR_CSS: &[u8] = include_bytes!("../../../web/doc-editor/dist/editor.css");
    const PROJECT_PREVIEW_INSPECTOR_JS: &str =
        include_str!("../../../assets/web/project-preview-inspector.js");
    const PROJECT_PREVIEW_AGENT_JS: &str =
        include_str!("../../../assets/web/project-preview-agent.js");
    const PROJECT_PREVIEW_CONSOLE_JS: &str =
        include_str!("../../../assets/web/project-preview-console.js");
    const PROJECT_PREVIEW_CONTENT_WORLD: &str = "Choro Project Preview";
    const PROJECT_PREVIEW_MESSAGE_HANDLER: &str = "choroPreview";
    const PROJECT_PREVIEW_CONSOLE_MESSAGE_HANDLER: &str = "choroPreviewConsole";
    const MAX_PROJECT_PREVIEW_MESSAGE_BYTES: usize = 256 * 1024;
    const MAX_PROJECT_PREVIEW_CONSOLE_MESSAGE_BYTES: usize = 32 * 1024;
    static WEBKIT_SURFACE_ACTIVE: AtomicBool = AtomicBool::new(false);

    struct ProjectPreviewMessageHandlerIvars {
        project_id: ProjectId,
        messages: Rc<RefCell<VecDeque<ProjectPreviewMessage>>>,
        app: WebPreviewWake,
    }

    fn decode_project_preview_message(
        body: &str,
    ) -> Result<ProjectPreviewInspectorMessage, String> {
        if body.len() > MAX_PROJECT_PREVIEW_MESSAGE_BYTES {
            return Err("The Preview review message was too large to send.".to_string());
        }
        serde_json::from_str(body)
            .map_err(|error| format!("The Preview review message was invalid: {error}"))
    }

    fn decode_project_preview_console_entry(
        body: &str,
    ) -> Result<ProjectPreviewConsoleEntry, String> {
        if body.len() > MAX_PROJECT_PREVIEW_CONSOLE_MESSAGE_BYTES {
            return Err("The Preview console message was too large.".to_string());
        }
        serde_json::from_str(body)
            .map_err(|error| format!("The Preview console message was invalid: {error}"))
    }

    fn bind_project_preview_message(
        project_id: ProjectId,
        message: ProjectPreviewInspectorMessage,
    ) -> ProjectPreviewMessage {
        match message {
            ProjectPreviewInspectorMessage::ToggleFocusMode => {
                ProjectPreviewMessage::ToggleFocusMode { project_id }
            }
            ProjectPreviewInspectorMessage::Cancelled => {
                ProjectPreviewMessage::Cancelled { project_id }
            }
            ProjectPreviewInspectorMessage::SelectionReady { target_kind } => {
                ProjectPreviewMessage::SelectionReady {
                    project_id,
                    target_kind,
                }
            }
            ProjectPreviewInspectorMessage::SubmitReview {
                comment,
                target_kind,
                element,
                area,
            } => ProjectPreviewMessage::SubmitReview {
                project_id,
                comment,
                target_kind,
                element,
                area,
            },
            ProjectPreviewInspectorMessage::AgentActionResult {
                command_id,
                success,
                capture,
                result,
                error,
            } => ProjectPreviewMessage::AgentActionResult {
                project_id,
                command_id,
                success,
                capture,
                result_json: serde_json::to_string(
                    &result.unwrap_or_else(|| serde_json::json!({})),
                )
                .unwrap_or_else(|_| "{}".to_string()),
                error,
            },
            ProjectPreviewInspectorMessage::AgentNavigationReady { command_id, result } => {
                ProjectPreviewMessage::AgentNavigationReady {
                    project_id,
                    command_id,
                    result_json: serde_json::to_string(&result)
                        .unwrap_or_else(|_| "{}".to_string()),
                }
            }
            ProjectPreviewInspectorMessage::AgentNavigationSettled { command_id, url } => {
                ProjectPreviewMessage::AgentNavigationSettled {
                    project_id,
                    command_id,
                    url,
                }
            }
            ProjectPreviewInspectorMessage::AgentNativeInputRequest {
                command_id,
                action,
                x,
                y,
                key,
                code,
                meta,
                control,
                alt,
                shift,
            } => ProjectPreviewMessage::AgentNativeInputRequest {
                project_id,
                command_id,
                action,
                x,
                y,
                key,
                code,
                meta,
                control,
                alt,
                shift,
            },
        }
    }

    fn enqueue_project_preview_message(
        messages: &Rc<RefCell<VecDeque<ProjectPreviewMessage>>>,
        app: &WebPreviewWake,
        message: ProjectPreviewMessage,
    ) {
        let Ok(mut messages) = messages.try_borrow_mut() else {
            eprintln!("project preview inspector message queue was busy");
            return;
        };
        messages.push_back(message);
        drop(messages);
        let _ = app.refresh();
    }

    fn project_preview_message_live_url(
        message: &ProjectPreviewMessage,
    ) -> Option<(ProjectId, &str)> {
        match message {
            ProjectPreviewMessage::AgentPageLoad {
                project_id, url, ..
            }
            | ProjectPreviewMessage::AgentNavigationSettled {
                project_id, url, ..
            } => Some((*project_id, url)),
            _ => None,
        }
    }

    define_class!(
        #[unsafe(super(NSObject))]
        #[name = "ChoroProjectPreviewMessageHandler"]
        #[thread_kind = MainThreadOnly]
        #[ivars = ProjectPreviewMessageHandlerIvars]
        struct ProjectPreviewMessageHandler;

        unsafe impl NSObjectProtocol for ProjectPreviewMessageHandler {}

        unsafe impl WKScriptMessageHandler for ProjectPreviewMessageHandler {
            #[unsafe(method(userContentController:didReceiveScriptMessage:))]
            fn did_receive(
                this: &ProjectPreviewMessageHandler,
                _controller: &WKUserContentController,
                message: &WKScriptMessage,
            ) {
                // Do not inspect the sending frame URL here. Wry's standard IPC
                // delegate unwraps that URL and aborts if WebKit reports none.
                let body = unsafe { message.body() };
                let Ok(body) = body.downcast::<NSString>() else {
                    eprintln!("project preview inspector sent a non-string message");
                    enqueue_project_preview_message(
                        &this.ivars().messages,
                        &this.ivars().app,
                        ProjectPreviewMessage::CaptureFailed {
                            project_id: this.ivars().project_id,
                            message: "The Preview review could not be read. Please try again."
                                .to_string(),
                        },
                    );
                    return;
                };
                let body = body.to_string();
                let message = match decode_project_preview_message(&body) {
                    Ok(message) => bind_project_preview_message(this.ivars().project_id, message),
                    Err(error) => {
                        eprintln!("invalid project preview inspector message: {error}");
                        enqueue_project_preview_message(
                            &this.ivars().messages,
                            &this.ivars().app,
                            ProjectPreviewMessage::CaptureFailed {
                                project_id: this.ivars().project_id,
                                message: error,
                            },
                        );
                        return;
                    }
                };
                enqueue_project_preview_message(&this.ivars().messages, &this.ivars().app, message);
            }
        }
    );

    struct ProjectPreviewConsoleMessageHandlerIvars {
        project_id: ProjectId,
        messages: Rc<RefCell<VecDeque<ProjectPreviewMessage>>>,
        app: WebPreviewWake,
    }

    define_class!(
        #[unsafe(super(NSObject))]
        #[name = "ChoroProjectPreviewConsoleMessageHandler"]
        #[thread_kind = MainThreadOnly]
        #[ivars = ProjectPreviewConsoleMessageHandlerIvars]
        struct ProjectPreviewConsoleMessageHandler;

        unsafe impl NSObjectProtocol for ProjectPreviewConsoleMessageHandler {}

        unsafe impl WKScriptMessageHandler for ProjectPreviewConsoleMessageHandler {
            #[unsafe(method(userContentController:didReceiveScriptMessage:))]
            fn did_receive(
                this: &ProjectPreviewConsoleMessageHandler,
                _controller: &WKUserContentController,
                message: &WKScriptMessage,
            ) {
                let body = unsafe { message.body() };
                let Ok(body) = body.downcast::<NSString>() else {
                    eprintln!("project preview console sent a non-string message");
                    return;
                };
                let entry = match decode_project_preview_console_entry(&body.to_string()) {
                    Ok(entry) => entry,
                    Err(error) => {
                        eprintln!("invalid project preview console message: {error}");
                        return;
                    }
                };
                enqueue_project_preview_message(
                    &this.ivars().messages,
                    &this.ivars().app,
                    ProjectPreviewMessage::ConsoleEntry {
                        project_id: this.ivars().project_id,
                        entry,
                    },
                );
            }
        }
    );

    impl ProjectPreviewMessageHandler {
        fn install(
            controller: &WKUserContentController,
            world: &WKContentWorld,
            project_id: ProjectId,
            messages: Rc<RefCell<VecDeque<ProjectPreviewMessage>>>,
            app: WebPreviewWake,
            main_thread: MainThreadMarker,
        ) {
            let handler =
                main_thread
                    .alloc::<Self>()
                    .set_ivars(ProjectPreviewMessageHandlerIvars {
                        project_id,
                        messages,
                        app,
                    });
            let handler: Retained<Self> = unsafe { msg_send![super(handler), init] };
            let protocol_handler = ProtocolObject::from_ref(&*handler);
            let user_script = unsafe {
                WKUserScript::initWithSource_injectionTime_forMainFrameOnly_inContentWorld(
                    main_thread.alloc::<WKUserScript>(),
                    &NSString::from_str(PROJECT_PREVIEW_INSPECTOR_JS),
                    WKUserScriptInjectionTime::AtDocumentStart,
                    true,
                    world,
                )
            };
            let agent_script = unsafe {
                WKUserScript::initWithSource_injectionTime_forMainFrameOnly_inContentWorld(
                    main_thread.alloc::<WKUserScript>(),
                    &NSString::from_str(PROJECT_PREVIEW_AGENT_JS),
                    WKUserScriptInjectionTime::AtDocumentStart,
                    true,
                    &world,
                )
            };
            unsafe {
                // WKUserContentController retains the handler for its lifetime.
                controller.addScriptMessageHandler_contentWorld_name(
                    protocol_handler,
                    world,
                    &NSString::from_str(PROJECT_PREVIEW_MESSAGE_HANDLER),
                );
                controller.addUserScript(&user_script);
                controller.addUserScript(&agent_script);
            }
        }
    }

    impl ProjectPreviewConsoleMessageHandler {
        fn install(
            controller: &WKUserContentController,
            page_world: &WKContentWorld,
            project_id: ProjectId,
            messages: Rc<RefCell<VecDeque<ProjectPreviewMessage>>>,
            app: WebPreviewWake,
            main_thread: MainThreadMarker,
        ) {
            let handler =
                main_thread
                    .alloc::<Self>()
                    .set_ivars(ProjectPreviewConsoleMessageHandlerIvars {
                        project_id,
                        messages,
                        app,
                    });
            let handler: Retained<Self> = unsafe { msg_send![super(handler), init] };
            let protocol_handler = ProtocolObject::from_ref(&*handler);
            let user_script = unsafe {
                WKUserScript::initWithSource_injectionTime_forMainFrameOnly_inContentWorld(
                    main_thread.alloc::<WKUserScript>(),
                    &NSString::from_str(PROJECT_PREVIEW_CONSOLE_JS),
                    WKUserScriptInjectionTime::AtDocumentStart,
                    true,
                    page_world,
                )
            };
            unsafe {
                // This page-world bridge accepts console entries only. Keeping
                // it separate prevents previewed code from forging inspector or
                // agent-control messages that live in Choro's isolated world.
                controller.addScriptMessageHandler_contentWorld_name(
                    protocol_handler,
                    page_world,
                    &NSString::from_str(PROJECT_PREVIEW_CONSOLE_MESSAGE_HANDLER),
                );
                controller.addUserScript(&user_script);
            }
        }
    }

    fn project_preview_content_world(main_thread: MainThreadMarker) -> Retained<WKContentWorld> {
        unsafe {
            WKContentWorld::worldWithName(
                &NSString::from_str(PROJECT_PREVIEW_CONTENT_WORLD),
                main_thread,
            )
        }
    }

    fn evaluate_project_preview_script(webview: &WebView, script: &str) -> Result<(), String> {
        let Some(main_thread) = MainThreadMarker::new() else {
            return Err("Preview review controls must run on the main thread.".to_string());
        };
        let world = project_preview_content_world(main_thread);
        unsafe {
            webview
                .webview()
                .evaluateJavaScript_inFrame_inContentWorld_completionHandler(
                    &NSString::from_str(script),
                    None,
                    &world,
                    None,
                );
        }
        Ok(())
    }

    fn project_preview_window_point(webview: &WebView, x: f64, y: f64) -> Result<NSPoint, String> {
        let view = webview.webview();
        let bounds = view.bounds();
        if !x.is_finite()
            || !y.is_finite()
            || x < 0.0
            || y < 0.0
            || x > bounds.size.width
            || y > bounds.size.height
        {
            return Err("The Preview input point is outside the visible WebView.".to_string());
        }
        let local_y = if view.isFlipped() {
            bounds.origin.y + y
        } else {
            bounds.origin.y + bounds.size.height - y
        };
        Ok(view.convertPoint_toView(NSPoint::new(bounds.origin.x + x, local_y), None))
    }

    fn send_project_preview_native_click(webview: &WebView, x: f64, y: f64) -> Result<(), String> {
        let view = webview.webview();
        let window = view
            .window()
            .ok_or_else(|| "The Preview WebView is not attached to a window.".to_string())?;
        let point = project_preview_window_point(webview, x, y)?;
        let timestamp = NSProcessInfo::processInfo().systemUptime();
        for (event_type, pressure) in [
            (NSEventType::MouseMoved, 0.0),
            (NSEventType::LeftMouseDown, 1.0),
            (NSEventType::LeftMouseUp, 0.0),
        ] {
            let event = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
                event_type,
                point,
                NSEventModifierFlags::empty(),
                timestamp,
                window.windowNumber(),
                None,
                0,
                1,
                pressure,
            )
            .ok_or_else(|| "AppKit could not create the Preview mouse event.".to_string())?;
            window.sendEvent(&event);
        }
        Ok(())
    }

    fn preview_key_event_data(key: &str, code: Option<&str>) -> (String, u16) {
        let named = match key {
            "Enter" | "Return" => Some(("\r", 36)),
            "Tab" => Some(("\t", 48)),
            " " | "Space" | "Spacebar" => Some((" ", 49)),
            "Backspace" => Some(("\u{8}", 51)),
            "Escape" | "Esc" => Some(("\u{1b}", 53)),
            "ArrowLeft" => Some(("\u{f702}", 123)),
            "ArrowRight" => Some(("\u{f703}", 124)),
            "ArrowDown" => Some(("\u{f701}", 125)),
            "ArrowUp" => Some(("\u{f700}", 126)),
            "Home" => Some(("\u{f729}", 115)),
            "End" => Some(("\u{f72b}", 119)),
            "PageUp" => Some(("\u{f72c}", 116)),
            "PageDown" => Some(("\u{f72d}", 121)),
            "Delete" => Some(("\u{f728}", 117)),
            _ => None,
        };
        if let Some((characters, key_code)) = named {
            return (characters.to_string(), key_code);
        }
        let key_code = match code.unwrap_or_default() {
            "KeyA" => 0,
            "KeyS" => 1,
            "KeyD" => 2,
            "KeyF" => 3,
            "KeyH" => 4,
            "KeyG" => 5,
            "KeyZ" => 6,
            "KeyX" => 7,
            "KeyC" => 8,
            "KeyV" => 9,
            "KeyB" => 11,
            "KeyQ" => 12,
            "KeyW" => 13,
            "KeyE" => 14,
            "KeyR" => 15,
            "KeyY" => 16,
            "KeyT" => 17,
            "Digit1" => 18,
            "Digit2" => 19,
            "Digit3" => 20,
            "Digit4" => 21,
            "Digit6" => 22,
            "Digit5" => 23,
            "Digit9" => 25,
            "Digit7" => 26,
            "Digit8" => 28,
            "Digit0" => 29,
            "KeyO" => 31,
            "KeyU" => 32,
            "KeyI" => 34,
            "KeyP" => 35,
            "KeyL" => 37,
            "KeyJ" => 38,
            "KeyK" => 40,
            "KeyN" => 45,
            "KeyM" => 46,
            _ => 0,
        };
        (key.to_string(), key_code)
    }

    fn send_project_preview_native_key(
        webview: &WebView,
        key: &str,
        code: Option<&str>,
        meta: bool,
        control: bool,
        alt: bool,
        shift: bool,
    ) -> Result<(), String> {
        let view = webview.webview();
        let window = view
            .window()
            .ok_or_else(|| "The Preview WebView is not attached to a window.".to_string())?;
        let (characters, key_code) = preview_key_event_data(key, code);
        let characters = NSString::from_str(&characters);
        let timestamp = NSProcessInfo::processInfo().systemUptime();
        let mut flags = NSEventModifierFlags::empty();
        flags.set(NSEventModifierFlags::Command, meta);
        flags.set(NSEventModifierFlags::Control, control);
        flags.set(NSEventModifierFlags::Option, alt);
        flags.set(NSEventModifierFlags::Shift, shift);
        for event_type in [NSEventType::KeyDown, NSEventType::KeyUp] {
            let event = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
                event_type,
                NSPoint::new(0.0, 0.0),
                flags,
                timestamp,
                window.windowNumber(),
                None,
                &characters,
                &characters,
                false,
                key_code,
            )
            .ok_or_else(|| "AppKit could not create the Preview key event.".to_string())?;
            window.sendEvent(&event);
        }
        Ok(())
    }

    pub struct WebPreviewHost {
        active: Option<Active>,
        pending: Option<WebPreviewIntent>,
        inline_placement: super::InlinePlacement,
        suspended: bool,
        overlay_suspended: bool,
        overlays: super::NativeOverlays,
        messages: Rc<RefCell<VecDeque<DocEditorMessage>>>,
        preview_messages: Rc<RefCell<VecDeque<ProjectPreviewMessage>>>,
        project_preview_live_urls: Rc<RefCell<HashMap<ProjectId, String>>>,
        app: WebPreviewWake,
    }

    struct Active {
        intent: WebPreviewIntent,
        webview: WebSurface,
        bounds: Bounds<Pixels>,
        doc_editor_ready: bool,
    }

    struct WebKitLease;

    impl WebKitLease {
        fn acquire() -> Result<Self, String> {
            WEBKIT_SURFACE_ACTIVE
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .map(|_| Self)
                .map_err(|_| {
                    "Choro already has a live WebKit surface; close it before opening another."
                        .to_string()
                })
        }
    }

    impl Drop for WebKitLease {
        fn drop(&mut self) {
            WEBKIT_SURFACE_ACTIVE.store(false, Ordering::Release);
        }
    }

    struct ProjectPreviewHandlers {
        controller: Retained<WKUserContentController>,
        inspector_world: Retained<WKContentWorld>,
        page_world: Retained<WKContentWorld>,
    }

    struct WebKitSurface {
        webview: WebView,
        keyboard_monitor: Option<Retained<AnyObject>>,
        project_preview_handlers: Option<ProjectPreviewHandlers>,
        // Declared last so the exclusivity slot is released only after WebView drops.
        _lease: WebKitLease,
    }

    impl WebKitSurface {
        fn new(webview: WebView, lease: WebKitLease) -> Self {
            Self::with_keyboard_routing(webview, lease, false)
        }

        fn with_keyboard_routing(webview: WebView, lease: WebKitLease, studio: bool) -> Self {
            let keyboard_monitor = install_preview_keyboard_monitor(&webview, studio);
            Self {
                webview,
                keyboard_monitor,
                project_preview_handlers: None,
                _lease: lease,
            }
        }

        fn project_preview(
            webview: WebView,
            lease: WebKitLease,
            handlers: ProjectPreviewHandlers,
        ) -> Self {
            let mut surface = Self::new(webview, lease);
            surface.project_preview_handlers = Some(handlers);
            surface
        }
    }

    impl Drop for WebKitSurface {
        fn drop(&mut self) {
            unsafe {
                if let Some(monitor) = self.keyboard_monitor.take() {
                    NSEvent::removeMonitor(&monitor);
                }
                if let Some(handlers) = self.project_preview_handlers.as_ref() {
                    handlers
                        .controller
                        .removeScriptMessageHandlerForName_contentWorld(
                            &NSString::from_str(PROJECT_PREVIEW_MESSAGE_HANDLER),
                            &handlers.inspector_world,
                        );
                    handlers
                        .controller
                        .removeScriptMessageHandlerForName_contentWorld(
                            &NSString::from_str(PROJECT_PREVIEW_CONSOLE_MESSAGE_HANDLER),
                            &handlers.page_world,
                        );
                }

                let manager = self.webview.manager();
                manager.removeAllUserScripts();
                let native = self.webview.webview();
                native.stopLoading();
                native.setNavigationDelegate(None);
                native.setUIDelegate(None);
            }
        }
    }

    fn preview_edit_action(key: &str, flags: NSEventModifierFlags, studio: bool) -> Option<objc2::runtime::Sel> {
        // Studio owns document undo in JavaScript. Let keyDown reach that
        // handler (and native field undo when it declines), instead of calling
        // WebKit's unrelated text undo manager before the DOM sees the key.
        if studio && (key.eq_ignore_ascii_case("z") || key.eq_ignore_ascii_case("y")) {
            return None;
        }
        let modifiers = flags
            & (NSEventModifierFlags::Command
                | NSEventModifierFlags::Control
                | NSEventModifierFlags::Option
                | NSEventModifierFlags::Shift);
        if modifiers == (NSEventModifierFlags::Command | NSEventModifierFlags::Shift)
            && key.eq_ignore_ascii_case("z")
        {
            return Some(sel!(redo:));
        }
        if modifiers != NSEventModifierFlags::Command {
            return None;
        }
        match key.to_ascii_lowercase().as_str() {
            "a" => Some(sel!(selectAll:)),
            "c" => Some(sel!(copy:)),
            "x" => Some(sel!(cut:)),
            "v" => Some(sel!(paste:)),
            "z" => Some(sel!(undo:)),
            "y" => Some(sel!(redo:)),
            _ => None,
        }
    }

    fn is_preview_editing_key(key: &str, flags: NSEventModifierFlags) -> bool {
        // Text, navigation, Option dead keys and Control editing commands belong
        // to the native first responder. Leave application Command shortcuts
        // (Quit, Close, window switching, etc.) on AppKit's normal menu path.
        !flags.contains(NSEventModifierFlags::Command)
            || matches!(
                key.to_ascii_lowercase().as_str(),
                "a" | "c" | "x" | "v" | "z" | "y" | "s"
            )
            || matches!(
                key,
                "\r" | "\n" | "\t" | "\u{8}" | "\u{7f}" | "\u{f700}" | "\u{f701}" | "\u{f702}" | "\u{f703}"
            )
    }

    fn install_preview_keyboard_monitor(webview: &WebView, studio: bool) -> Option<Retained<AnyObject>> {
        let view: Retained<NSView> = webview.webview().into_super().into_super();
        let handler = RcBlock::new(move |event: std::ptr::NonNull<NSEvent>| unsafe {
            let native_event = event.as_ref();
            let key = native_event
                .charactersIgnoringModifiers()
                .map(|key| key.to_string())
                .unwrap_or_default();
            // Other surfaces retain their existing paste-only workaround;
            // editors such as Docs own their own JavaScript shortcut handling.
            let route = if studio {
                is_preview_editing_key(&key, native_event.modifierFlags())
            } else {
                native_event.r#type() == NSEventType::KeyDown
                    && preview_edit_action(&key, native_event.modifierFlags(), false) == Some(sel!(paste:))
            };
            if !route
                || view.isHiddenOrHasHiddenAncestor()
            {
                return event.as_ptr();
            }
            let Some(window) = view.window() else {
                return event.as_ptr();
            };
            if native_event.window(view.mtm()).as_ref() != Some(&window) {
                return event.as_ptr();
            }
            let Some(responder) = window.firstResponder() else {
                return event.as_ptr();
            };
            let Some(focused_view) = responder.downcast_ref::<NSView>() else {
                return event.as_ptr();
            };
            if !focused_view.isDescendantOf(&view) {
                return event.as_ptr();
            }

            // AppKit offers key equivalents to GPUIView, whose logical focus
            // can still point at the agent composer after a click in WebKit.
            // Deliver the original event to the actual native first responder
            // before that happens. Using keyDown (not insertText or JS) keeps
            // selection, IME/dead keys, repeats and DOM keyboard events native.
            if native_event.r#type() == NSEventType::KeyUp {
                responder.keyUp(native_event);
            } else if let Some(action) = preview_edit_action(&key, native_event.modifierFlags(), studio) {
                // AppKit normally runs these through the Edit menu, after key
                // equivalents. Use the native responder's action directly so
                // GPUI's stale composer cannot claim them first.
                if action == sel!(undo:) || action == sel!(redo:) {
                    if let Some(manager) = responder.undoManager() {
                        if action == sel!(undo:) {
                            manager.undo();
                        } else {
                            manager.redo();
                        }
                    }
                } else if responder.respondsToSelector(action) {
                    // These standard editing actions take a nullable sender
                    // and return void (not an ARC-retained object).
                    let _: () = (&*responder).send_message(action, (std::ptr::null::<AnyObject>(),));
                }
            } else {
                responder.keyDown(native_event);
            }
            std::ptr::null_mut()
        });
        unsafe {
            NSEvent::addLocalMonitorForEventsMatchingMask_handler(
                NSEventMask::KeyDown | NSEventMask::KeyUp,
                &handler,
            )
        }
    }

    enum WebSurface {
        WebKit(WebKitSurface),
    }

    impl WebSurface {
        fn set_visible(&self, visible: bool) -> Result<(), String> {
            match self {
                Self::WebKit(surface) => surface
                    .webview
                    .set_visible(visible)
                    .map_err(|error| error.to_string()),
            }
        }

        fn set_bounds(&self, bounds: Bounds<Pixels>) -> Result<(), String> {
            match self {
                Self::WebKit(surface) => surface
                    .webview
                    .set_bounds(to_rect(bounds))
                    .map_err(|error| error.to_string()),
            }
        }

        fn load_url(&self, url: &str) -> Result<(), String> {
            match self {
                Self::WebKit(surface) => surface
                    .webview
                    .load_url(url)
                    .map_err(|error| error.to_string()),
            }
        }

        fn evaluate_script(&self, script: &str) -> Result<(), String> {
            match self {
                Self::WebKit(surface) => surface
                    .webview
                    .evaluate_script(script)
                    .map_err(|error| error.to_string()),
            }
        }

        fn reload_from_origin(&self) -> Result<(), String> {
            match self {
                Self::WebKit(surface) => {
                    // Reload the document WebKit is currently showing, not the
                    // configured Preview intent URL. `reloadFromOrigin` also
                    // revalidates resources so this acts like a real browser
                    // refresh while debugging local changes.
                    unsafe {
                        surface.webview.webview().reloadFromOrigin();
                    }
                    Ok(())
                }
            }
        }

        fn current_url(&self) -> Option<String> {
            match self {
                Self::WebKit(surface) => unsafe {
                    surface
                        .webview
                        .webview()
                        .URL()
                        .and_then(|url| url.absoluteString())
                        .map(|url| url.to_string())
                },
            }
        }

        fn webkit(&self) -> Option<&WebView> {
            match self {
                Self::WebKit(surface) => Some(&surface.webview),
            }
        }

    }

    impl WebPreviewHost {
        pub fn new(app: WebPreviewWake) -> Self {
            Self {
                active: None,
                pending: None,
                inline_placement: Default::default(),
                suspended: false,
                overlay_suspended: false,
                overlays: super::NativeOverlays::default(),
                messages: Rc::new(RefCell::new(VecDeque::new())),
                preview_messages: Rc::new(RefCell::new(VecDeque::new())),
                project_preview_live_urls: Rc::new(RefCell::new(HashMap::new())),
                app,
            }
        }

        /// Hide the native child while an app-level route such as Settings is
        /// active. Unlike `set_intent(None)`, suspension preserves the live
        /// editor/preview and restores it without a reload when the route closes.
        pub fn set_suspended(&mut self, suspended: bool) {
            if self.suspended == suspended {
                return;
            }
            self.suspended = suspended;
            if let Some(active) = self.active.as_ref() {
                let _ = active
                    .webview
                    .set_visible(surface_visible(self.is_hidden(), active.doc_editor_ready));
            }
        }

        /// Hide the native child while a GPUI popover overlaps its rectangle.
        /// This is separate from route suspension so closing a menu cannot
        /// accidentally reveal the WebView beneath an app-level modal.
        pub fn set_overlay_suspended(&mut self, suspended: bool) {
            if self.overlay_suspended == suspended {
                return;
            }
            self.overlay_suspended = suspended;
            if let Some(active) = self.active.as_ref() {
                let _ = active
                    .webview
                    .set_visible(surface_visible(self.is_hidden(), active.doc_editor_ready));
            }
        }

        pub fn is_route_suspended(&self) -> bool {
            self.suspended
        }

        fn is_hidden(&self) -> bool {
            self.suspended || self.overlay_suspended || self.overlays.hidden()
                || (self.inline_placement.hidden && self.active.as_ref().is_some_and(|active|
                    matches!(active.intent, WebPreviewIntent::Visualization { .. })))
        }

        fn update_overlay_visibility(&self) {
            if let Some(active) = self.active.as_ref() {
                let _ = active
                    .webview
                    .set_visible(surface_visible(self.is_hidden(), active.doc_editor_ready));
            }
        }

        pub fn set_modal_suspended(&mut self, suspended: bool) {
            if self.overlays.modal != suspended {
                self.overlays.modal = suspended;
                self.update_overlay_visibility();
            }
        }

        pub fn begin_popup(&mut self) {
            self.overlays.begin_popup();
            self.update_overlay_visibility();
        }

        pub fn end_popup(&mut self) {
            self.overlays.end_popup();
            self.update_overlay_visibility();
        }

        /// Select the single web surface. Cached regions can reuse their native
        /// placement across frames; navigation and suspension own its lifetime.
        pub fn set_intent(&mut self, intent: Option<WebPreviewIntent>) -> bool {
            let had_active = self.active.is_some();
            self.cache_active_project_preview_live_url();

            let Some(intent) = intent else {
                self.active = None;
                self.pending = None;
                return had_active;
            };

            if self
                .active
                .as_ref()
                .is_some_and(|active| same_surface(&active.intent, &intent))
            {
                if self.active.as_ref().map(|active| &active.intent) != Some(&intent) {
                    if let Some(active) = self.active.as_mut() {
                        sync_active_doc_editor(active, &intent);
                        sync_active_project_preview(active, &intent);
                        active.intent = intent.clone();
                    }
                }
                // Cached GPUI regions reuse placement without invoking a canvas
                // callback on every frame. Intent and suspension own visibility.
                return false;
            }

            if self.pending.as_ref() == Some(&intent) {
                return false;
            }

            self.active = None;
            self.pending = Some(intent);
            had_active
        }

        /// Drain editor IPC. Uploads are fulfilled here because the live
        /// WebView is needed to resolve the JavaScript promise; content changes
        /// are returned to the owning DocsState.
        pub fn take_doc_editor_messages(&mut self) -> Vec<DocEditorMessage> {
            let messages = self.messages.borrow_mut().drain(..).collect::<Vec<_>>();
            let mut changes = Vec::new();
            let mut latest_change = None;
            for message in messages {
                match message {
                    DocEditorMessage::Upload {
                        request_id,
                        name,
                        mime,
                        data_url,
                    } => self.fulfill_upload(request_id, name, mime, data_url),
                    DocEditorMessage::Change { path, document } => {
                        let mut matches_active_document = false;
                        if let Some(Active {
                            intent:
                                WebPreviewIntent::DocEditor {
                                    path: active_path,
                                    document: active_document,
                                    ..
                                },
                            ..
                        }) = self.active.as_mut()
                        {
                            if *active_path == path {
                                *active_document = document.clone();
                                matches_active_document = true;
                            }
                        }
                        if matches_active_document {
                            latest_change = Some((path, document));
                        }
                    }
                    DocEditorMessage::Ready { path } => {
                        let visible = !self.is_hidden();
                        if let Some(active) = self.active.as_mut() {
                            let matches_active_document = matches!(
                                &active.intent,
                                WebPreviewIntent::DocEditor { path: active_path, .. }
                                    if active_path == Path::new(&path)
                            );
                            if matches_active_document {
                                active.doc_editor_ready = true;
                                let _ = active.webview.set_visible(visible);
                            }
                        }
                    }
                    DocEditorMessage::Error { message } => {
                        eprintln!("document editor failed to start: {message}");
                    }
                    DocEditorMessage::OpenReference { path, target } => {
                        let matches_active_document = self.active.as_ref().is_some_and(|active| {
                            matches!(
                                &active.intent,
                                WebPreviewIntent::DocEditor { path: active_path, .. }
                                    if *active_path == path
                            )
                        });
                        if matches_active_document {
                            changes.push(DocEditorMessage::OpenReference { path, target });
                        }
                    }
                }
            }
            if let Some((path, document)) = latest_change {
                changes.insert(0, DocEditorMessage::Change { path, document });
            }
            changes
        }

        pub fn take_project_preview_messages(&mut self) -> Vec<ProjectPreviewMessage> {
            let messages = self
                .preview_messages
                .borrow_mut()
                .drain(..)
                .collect::<Vec<_>>();
            let mut live_urls = self.project_preview_live_urls.borrow_mut();
            for message in &messages {
                if let Some((project_id, url)) = project_preview_message_live_url(message) {
                    live_urls.insert(project_id, url.to_string());
                }
            }
            messages
        }








        pub fn set_project_preview_inspecting(&self, inspecting: bool) -> Result<(), String> {
            let Some(active) = self.active.as_ref() else {
                return Err("The project preview is still loading.".to_string());
            };
            if !matches!(active.intent, WebPreviewIntent::ProjectPreview { .. }) {
                return Err("There is no project preview open.".to_string());
            }
            let action = if inspecting { "activate" } else { "deactivate" };
            let Some(webview) = active.webview.webkit() else {
                return Err("Preview review is unavailable in this surface.".to_string());
            };
            evaluate_project_preview_script(
                webview,
                &format!("window.__choroProjectPreviewInspector?.{action}();"),
            )
            .map_err(|error| format!("Could not start Preview review: {error}"))
        }

        pub fn reload_project_preview(&self) -> Result<(), String> {
            let Some(active) = self.active.as_ref() else {
                return Err("The project preview is still loading.".to_string());
            };
            if !matches!(active.intent, WebPreviewIntent::ProjectPreview { .. }) {
                return Err("There is no project preview open.".to_string());
            }
            active
                .webview
                .reload_from_origin()
                .map_err(|error| format!("Could not reload Preview: {error}"))
        }

        pub fn navigate_project_preview_url(
            &self,
            project_id: ProjectId,
            url: &str,
        ) -> Result<(), String> {
            let Some(active) = self.active.as_ref() else {
                return Err("The project Preview is still loading.".to_string());
            };
            if !matches!(
                &active.intent,
                WebPreviewIntent::ProjectPreview {
                    project_id: active_project,
                    ..
                } if *active_project == project_id
            ) {
                return Err("That project's Preview is not active.".to_string());
            }
            let webview = active
                .webview
                .webkit()
                .ok_or_else(|| "Project Preview requires the WebKit surface.".to_string())?;
            if let Some(path) = project_preview_file_path(url) {
                load_project_preview_file(webview, &path);
                self.project_preview_live_urls
                    .borrow_mut()
                    .insert(project_id, url.to_string());
                return Ok(());
            }
            webview
                .load_url(url)
                .map_err(|error| format!("Could not open Preview URL: {error}"))?;
            self.project_preview_live_urls
                .borrow_mut()
                .insert(project_id, url.to_string());
            Ok(())
        }

        pub fn navigate_project_preview_history(&self, forward: bool) -> Result<(), String> {
            let Some(active) = self.active.as_ref() else {
                return Err("The project preview is still loading.".to_string());
            };
            if !matches!(active.intent, WebPreviewIntent::ProjectPreview { .. }) {
                return Err("There is no project preview open.".to_string());
            }
            active
                .webview
                .evaluate_script(if forward {
                    "history.forward();"
                } else {
                    "history.back();"
                })
                .map_err(|error| format!("Could not navigate Preview: {error}"))
        }

        pub fn project_preview_live_url(&self, project_id: ProjectId) -> Result<String, String> {
            let Some(active) = self.active.as_ref() else {
                return Err("The project Preview is still loading.".to_string());
            };
            if !matches!(
                &active.intent,
                WebPreviewIntent::ProjectPreview {
                    project_id: active_project,
                    ..
                } if *active_project == project_id
            ) {
                return Err("That project's Preview is not active.".to_string());
            }
            self.cache_active_project_preview_live_url();
            self.project_preview_live_urls
                .borrow()
                .get(&project_id)
                .cloned()
                .or_else(|| match &active.intent {
                    WebPreviewIntent::ProjectPreview { url, .. } => Some(url.clone()),
                    _ => None,
                })
                .ok_or_else(|| "The project Preview URL is not ready yet.".to_string())
        }

        /// Return the most recent live URL even when another project's webview
        /// is active. Keep page uses this to rebuild a torn-down project at the
        /// route the user was actually debugging.
        pub fn cached_project_preview_live_url(&self, project_id: ProjectId) -> Option<String> {
            self.cache_active_project_preview_live_url();
            self.project_preview_live_urls
                .borrow()
                .get(&project_id)
                .cloned()
        }

        pub fn is_project_preview_active(&self, project_id: ProjectId) -> bool {
            self.active.as_ref().is_some_and(|active| {
                matches!(
                    active.intent,
                    WebPreviewIntent::ProjectPreview {
                        project_id: active_project,
                        ..
                    } if active_project == project_id
                )
            })
        }

        fn cache_active_project_preview_live_url(&self) {
            let Some(active) = self.active.as_ref() else {
                return;
            };
            let WebPreviewIntent::ProjectPreview { project_id, .. } = &active.intent else {
                return;
            };
            let Some(url) = active.webview.current_url() else {
                return;
            };
            if url != "about:blank" {
                self.project_preview_live_urls
                    .borrow_mut()
                    .insert(*project_id, url);
            }
        }

        pub fn execute_project_preview_agent_command(
            &self,
            project_id: ProjectId,
            command_id: Uuid,
            action: &str,
            payload_json: &str,
            policy_json: &str,
        ) -> Result<(), String> {
            let Some(active) = self.active.as_ref() else {
                return Err("The project Preview is still loading.".to_string());
            };
            if !matches!(
                &active.intent,
                WebPreviewIntent::ProjectPreview {
                    project_id: active_project,
                    ..
                } if *active_project == project_id
            ) {
                return Err("That project's Preview is not active.".to_string());
            }
            let payload: serde_json::Value = serde_json::from_str(payload_json)
                .map_err(|error| format!("The Preview action payload is invalid: {error}"))?;
            let command = serde_json::json!({
                "id": command_id,
                "action": action,
                "payload": payload,
            });
            let command = serde_json::to_string(&command)
                .map_err(|error| format!("Could not encode Preview action: {error}"))?;
            let webview = active
                .webview
                .webkit()
                .ok_or_else(|| "Project Preview requires the WebKit surface.".to_string())?;
            // A freshly navigated WKWebView can paint before its document-start
            // user script is observable from a separately evaluated isolated-
            // world script. The helper is idempotent, so bootstrap it here as
            // well and make command readiness independent of WebKit timing.
            evaluate_project_preview_script(
                webview,
                &format!(
                    "{PROJECT_PREVIEW_AGENT_JS}\nwindow.__choroProjectPreviewAgent.execute({command}, {policy_json});"
                ),
            )
            .map_err(|error| format!("Could not control Preview: {error}"))
        }

        pub fn activate_project_preview_navigation(
            &self,
            project_id: ProjectId,
            command_id: Uuid,
        ) -> Result<(), String> {
            let Some(active) = self.active.as_ref() else {
                return Err("The project Preview is still loading.".to_string());
            };
            if !matches!(
                &active.intent,
                WebPreviewIntent::ProjectPreview {
                    project_id: active_project,
                    ..
                } if *active_project == project_id
            ) {
                return Err("That project's Preview is not active.".to_string());
            }
            let command_id = serde_json::to_string(&command_id.to_string())
                .map_err(|error| format!("Could not encode Preview navigation: {error}"))?;
            let webview = active
                .webview
                .webkit()
                .ok_or_else(|| "Project Preview requires the WebKit surface.".to_string())?;
            evaluate_project_preview_script(
                webview,
                &format!("window.__choroProjectPreviewAgent?.activateNavigation({command_id});"),
            )
            .map_err(|error| format!("Could not activate Preview navigation: {error}"))
        }

        pub fn cancel_project_preview_agent_command(
            &self,
            project_id: ProjectId,
            command_id: Uuid,
            reason: &str,
        ) -> Result<(), String> {
            let Some(active) = self.active.as_ref() else {
                return Ok(());
            };
            if !matches!(
                &active.intent,
                WebPreviewIntent::ProjectPreview {
                    project_id: active_project,
                    ..
                } if *active_project == project_id
            ) {
                return Ok(());
            }
            let command_id = serde_json::to_string(&command_id.to_string())
                .map_err(|error| format!("Could not encode Preview cancellation: {error}"))?;
            let reason = serde_json::to_string(reason)
                .map_err(|error| format!("Could not encode Preview cancellation: {error}"))?;
            let webview = active
                .webview
                .webkit()
                .ok_or_else(|| "Project Preview requires the WebKit surface.".to_string())?;
            evaluate_project_preview_script(
                webview,
                &format!("window.__choroProjectPreviewAgent?.cancel({command_id}, {reason});"),
            )
            .map_err(|error| format!("Could not reset Preview control: {error}"))
        }

        #[allow(clippy::too_many_arguments)]
        pub fn perform_project_preview_native_input(
            &self,
            project_id: ProjectId,
            command_id: Uuid,
            action: &str,
            x: Option<f64>,
            y: Option<f64>,
            key: Option<&str>,
            code: Option<&str>,
            meta: bool,
            control: bool,
            alt: bool,
            shift: bool,
        ) -> Result<(), String> {
            let Some(active) = self.active.as_ref() else {
                return Err("The project Preview is still loading.".to_string());
            };
            if !matches!(
                &active.intent,
                WebPreviewIntent::ProjectPreview {
                    project_id: active_project,
                    ..
                } if *active_project == project_id
            ) {
                return Err("That project's Preview is not active.".to_string());
            }
            let webview = active
                .webview
                .webkit()
                .ok_or_else(|| "Project Preview requires the WebKit surface.".to_string())?;

            let native_result = match action {
                "click" => send_project_preview_native_click(
                    webview,
                    x.ok_or_else(|| "Native Preview click is missing x.".to_string())?,
                    y.ok_or_else(|| "Native Preview click is missing y.".to_string())?,
                ),
                "key" => send_project_preview_native_key(
                    webview,
                    key.ok_or_else(|| "Native Preview key is missing.".to_string())?,
                    code,
                    meta,
                    control,
                    alt,
                    shift,
                ),
                other => Err(format!("Unsupported native Preview input: {other}")),
            };
            let success = native_result.is_ok();
            let error = native_result.err();
            let completion = serde_json::to_string(&serde_json::json!({
                "commandId": command_id,
                "success": success,
                "error": error,
            }))
            .map_err(|error| format!("Could not encode native Preview completion: {error}"))?;
            evaluate_project_preview_script(
                webview,
                &format!(
                    "window.__choroProjectPreviewAgent?.nativeCompleted({0}.commandId, {0}.success, {0}.error);",
                    completion
                ),
            )
            .map_err(|error| format!("Could not finish native Preview input: {error}"))
        }

        pub fn capture_project_preview_agent_snapshot(
            &self,
            project_id: ProjectId,
            command_id: Uuid,
            result_json: String,
        ) {
            let Some(active) = self.active.as_ref() else {
                enqueue_project_preview_message(
                    &self.preview_messages,
                    &self.app,
                    ProjectPreviewMessage::AgentSnapshotFailed {
                        project_id,
                        command_id,
                        message: "The project Preview is no longer active.".to_string(),
                    },
                );
                return;
            };
            if !matches!(
                &active.intent,
                WebPreviewIntent::ProjectPreview {
                    project_id: active_project,
                    ..
                } if *active_project == project_id
            ) {
                enqueue_project_preview_message(
                    &self.preview_messages,
                    &self.app,
                    ProjectPreviewMessage::AgentSnapshotFailed {
                        project_id,
                        command_id,
                        message: "That project's Preview is no longer active.".to_string(),
                    },
                );
                return;
            }

            let messages = self.preview_messages.clone();
            let app = self.app.clone();
            let Some(webview) = active.webview.webkit() else {
                enqueue_project_preview_message(
                    &self.preview_messages,
                    &self.app,
                    ProjectPreviewMessage::AgentSnapshotFailed {
                        project_id,
                        command_id,
                        message: "Project Preview requires the WebKit surface.".to_string(),
                    },
                );
                return;
            };
            let webview = webview.webview();
            let completion: RcBlock<dyn Fn(*mut NSImage, *mut NSError)> =
                RcBlock::new(move |image: *mut NSImage, error: *mut NSError| {
                    let message = match unsafe { image.as_ref() } {
                        Some(image) => match visualization_png_bytes(image) {
                            Ok(bytes) if bytes.len() <= 5 * 1024 * 1024 => {
                                ProjectPreviewMessage::AgentSnapshotReady {
                                    project_id,
                                    command_id,
                                    result_json: result_json.clone(),
                                    image_base64: base64::engine::general_purpose::STANDARD
                                        .encode(bytes),
                                }
                            }
                            Ok(_) => ProjectPreviewMessage::AgentSnapshotFailed {
                                project_id,
                                command_id,
                                message: "The Preview snapshot exceeds the 5 MB limit.".to_string(),
                            },
                            Err(message) => ProjectPreviewMessage::AgentSnapshotFailed {
                                project_id,
                                command_id,
                                message,
                            },
                        },
                        None if !error.is_null() => ProjectPreviewMessage::AgentSnapshotFailed {
                            project_id,
                            command_id,
                            message: "The desktop could not capture the Preview.".to_string(),
                        },
                        None => ProjectPreviewMessage::AgentSnapshotFailed {
                            project_id,
                            command_id,
                            message: "The Preview is not ready for a snapshot.".to_string(),
                        },
                    };
                    enqueue_project_preview_message(&messages, &app, message);
                });
            unsafe {
                webview.takeSnapshotWithConfiguration_completionHandler(None, &completion);
            }
        }

        #[allow(clippy::too_many_arguments)]
        pub fn capture_project_preview_review(
            &self,
            project_id: ProjectId,
            agent_id: uuid::Uuid,
            url: String,
            comment: String,
            target_kind: String,
            element: Option<ide_core::visual_review::VisualElementSelection>,
            area: Option<ide_core::visual_review::VisualAreaSelection>,
        ) {
            let Some(active) = self.active.as_ref() else {
                enqueue_project_preview_message(
                    &self.preview_messages,
                    &self.app,
                    ProjectPreviewMessage::CaptureFailed {
                        project_id,
                        message: "The project preview is still loading.".to_string(),
                    },
                );
                return;
            };
            if !matches!(
                &active.intent,
                WebPreviewIntent::ProjectPreview {
                    project_id: active_project,
                    ..
                } if *active_project == project_id
            ) {
                enqueue_project_preview_message(
                    &self.preview_messages,
                    &self.app,
                    ProjectPreviewMessage::CaptureFailed {
                        project_id,
                        message: "That project's Preview is no longer active.".to_string(),
                    },
                );
                return;
            }
            let Some(rect) = element
                .as_ref()
                .map(|selection| &selection.rect)
                .or_else(|| area.as_ref().map(|selection| &selection.rect))
            else {
                enqueue_project_preview_message(
                    &self.preview_messages,
                    &self.app,
                    ProjectPreviewMessage::CaptureFailed {
                        project_id,
                        message: "Select an element or image area first.".to_string(),
                    },
                );
                return;
            };

            let Some(webview) = active.webview.webkit() else {
                enqueue_project_preview_message(
                    &self.preview_messages,
                    &self.app,
                    ProjectPreviewMessage::CaptureFailed {
                        project_id,
                        message: "Preview capture is unavailable in this surface.".to_string(),
                    },
                );
                return;
            };
            let webview = webview.webview();
            let native_view = webview.as_super();
            let bounds = native_view.bounds();
            let x = rect.x.max(0.0).min(bounds.size.width);
            let css_y = rect.y.max(0.0).min(bounds.size.height);
            let width = rect.width.max(1.0).min(bounds.size.width - x);
            let height = rect.height.max(1.0).min(bounds.size.height - css_y);
            if width < 1.0 || height < 1.0 {
                enqueue_project_preview_message(
                    &self.preview_messages,
                    &self.app,
                    ProjectPreviewMessage::CaptureFailed {
                        project_id,
                        message: "The selected review is outside the visible Preview.".to_string(),
                    },
                );
                return;
            }
            let y = if native_view.isFlipped() {
                css_y
            } else {
                bounds.size.height - (css_y + height)
            };
            let Some(main_thread) = MainThreadMarker::new() else {
                enqueue_project_preview_message(
                    &self.preview_messages,
                    &self.app,
                    ProjectPreviewMessage::CaptureFailed {
                        project_id,
                        message: "Preview capture must run on the main thread.".to_string(),
                    },
                );
                return;
            };
            let configuration = unsafe { WKSnapshotConfiguration::new(main_thread) };
            unsafe {
                configuration.setRect(CGRect::new(
                    CGPoint::new(x, y.max(0.0)),
                    CGSize::new(width, height),
                ));
                configuration.setAfterScreenUpdates(true);
            }
            let _ = evaluate_project_preview_script(
                active
                    .webview
                    .webkit()
                    .expect("project preview always uses WebKit"),
                "window.__choroProjectPreviewInspector?.prepareSnapshot();",
            );

            let messages = self.preview_messages.clone();
            let app = self.app.clone();
            let completion: RcBlock<dyn Fn(*mut NSImage, *mut NSError)> =
                RcBlock::new(move |image: *mut NSImage, error: *mut NSError| {
                    let message = match unsafe { image.as_ref() } {
                        Some(image) => match visualization_png_bytes(image) {
                            Ok(bytes) => ProjectPreviewMessage::CaptureReady {
                                project_id,
                                agent_id,
                                url: url.clone(),
                                comment: comment.clone(),
                                target_kind: target_kind.clone(),
                                element: element.clone(),
                                area: area.clone(),
                                image_base64: base64::engine::general_purpose::STANDARD
                                    .encode(bytes),
                            },
                            Err(message) => ProjectPreviewMessage::CaptureFailed {
                                project_id,
                                message,
                            },
                        },
                        None if !error.is_null() => ProjectPreviewMessage::CaptureFailed {
                            project_id,
                            message: "The desktop could not capture the selected Preview area."
                                .to_string(),
                        },
                        None => ProjectPreviewMessage::CaptureFailed {
                            project_id,
                            message: "The selected Preview area is not ready yet.".to_string(),
                        },
                    };
                    enqueue_project_preview_message(&messages, &app, message);
                });
            unsafe {
                webview.takeSnapshotWithConfiguration_completionHandler(
                    Some(&configuration),
                    &completion,
                );
            }
        }

        pub fn finish_project_preview_submission(&self, project_id: ProjectId, succeeded: bool) {
            let Some(active) = self.active.as_ref() else {
                return;
            };
            if !matches!(
                &active.intent,
                WebPreviewIntent::ProjectPreview {
                    project_id: active_project,
                    ..
                } if *active_project == project_id
            ) {
                return;
            }
            let action = if succeeded {
                "submitted"
            } else {
                "submissionFailed"
            };
            let Some(webview) = active.webview.webkit() else {
                return;
            };
            let _ = evaluate_project_preview_script(
                webview,
                &format!("window.__choroProjectPreviewInspector?.{action}();"),
            );
        }

        fn fulfill_upload(&self, request_id: String, name: String, mime: String, data_url: String) {
            let Some(Active {
                intent: WebPreviewIntent::DocEditor { path, .. },
                webview,
                ..
            }) = self.active.as_ref()
            else {
                return;
            };
            let result = persist_doc_upload(path, &name, &mime, &data_url);
            let (method, value) = match result {
                Ok(url) => ("resolveUpload", url),
                Err(error) => ("rejectUpload", error),
            };
            let request = serde_json::to_string(&request_id).unwrap_or_else(|_| "\"\"".into());
            let value = serde_json::to_string(&value).unwrap_or_else(|_| "\"\"".into());
            let _ = webview.evaluate_script(&format!(
                "window.choroEditor?.{method}({request}, {value});"
            ));
        }

        pub fn begin_transcript_layout(&mut self) {
            let intent = self.active.as_ref().map(|active| &active.intent).or(self.pending.as_ref());
            if matches!(intent, Some(WebPreviewIntent::Visualization { .. })) {
                self.inline_placement.begin();
            }
        }

        pub fn finish_transcript_layout(&mut self) {
            let was_hidden = self.inline_placement.hidden;
            self.inline_placement.finish();
            if was_hidden != self.inline_placement.hidden { self.update_overlay_visibility(); }
        }

        /// Build or reposition the selected web surface inside a GPUI-reserved
        /// rectangle. Only a visible chat row calls this method.
        pub fn place(&mut self, bounds: Bounds<Pixels>, window: &Window) {
            let intent = self.active.as_ref().map(|active| &active.intent).or(self.pending.as_ref());
            if matches!(intent, Some(WebPreviewIntent::Visualization { .. })) {
                let visible = bounds.intersect(&window.content_mask().bounds);
                if visible.size.width <= gpui::px(0.) || visible.size.height <= gpui::px(0.) { return; }
                self.inline_placement.place();
            }
            if self.is_hidden() {
                if let Some(active) = self.active.as_ref() {
                    let _ = active.webview.set_visible(false);
                }
                return;
            }
            if let Some(intent) = self.pending.take() {
                match build(
                    &intent,
                    bounds,
                    window,
                    self.messages.clone(),
                    self.preview_messages.clone(),
                    self.project_preview_live_urls.clone(),
                    self.app.clone(),
                ) {
                    Ok(webview) => {
                        if let WebPreviewIntent::ProjectPreview {
                            project_id, url, ..
                        } = &intent
                        {
                            self.project_preview_live_urls
                                .borrow_mut()
                                .insert(*project_id, url.clone());
                        }
                        let doc_editor_ready =
                            !matches!(intent, WebPreviewIntent::DocEditor { .. });
                        self.active = Some(Active {
                            intent,
                            webview,
                            bounds,
                            doc_editor_ready,
                        });
                    }
                    Err(error) => eprintln!("web preview build failed: {error}"),
                }
            } else if let Some(active) = &mut self.active {
                let _ = active
                    .webview
                    .set_visible(surface_visible(false, active.doc_editor_ready));
                if active.bounds != bounds {
                    active.bounds = bounds;
                    let _ = active.webview.set_bounds(bounds);
                }
            }
        }

        pub fn doc_editor_ready_for(&self, path: &Path) -> bool {
            self.active.as_ref().is_some_and(|active| {
                active.doc_editor_ready
                    && matches!(
                        &active.intent,
                        WebPreviewIntent::DocEditor { path: active_path, .. }
                            if active_path == path
                    )
            })
        }

        /// Copy the visible pixels of the live visualization to the macOS
        /// image clipboard. WKWebView performs the snapshot asynchronously and
        /// copies the completion block for the duration of the request.
        pub fn studio_reply(&self, value: &serde_json::Value) {
            if let Some(active) = self
                .active
                .as_ref()
                .filter(|a| matches!(a.intent, WebPreviewIntent::Studio { .. } | WebPreviewIntent::StudioCanvas { .. }))
            {
                let _ = active
                    .webview
                    .evaluate_script(&format!("window.choroStudioReply?.({value})"));
            }
        }
        pub fn canvas_reply(&self,value:&serde_json::Value){
            if let Some(active)=self.active.as_ref().filter(|a|matches!(a.intent,WebPreviewIntent::StudioCanvas{..})){
                let _=active.webview.evaluate_script(&format!("window.choroCanvasReply?.({value})"));
            }
        }
        pub fn copy_active_visualization_image(&self) -> Result<(), String> {
            let Some(active) = self.active.as_ref() else {
                return Err("The visualization is not ready yet.".to_string());
            };
            if !matches!(active.intent, WebPreviewIntent::Visualization { .. }) {
                return Err("There is no live visualization to copy.".to_string());
            }

            let Some(webview) = active.webview.webkit() else {
                return Err("The visualization cannot be captured from this surface.".to_string());
            };
            let webview = webview.webview();
            let completion: RcBlock<dyn Fn(*mut NSImage, *mut NSError)> =
                RcBlock::new(move |image: *mut NSImage, error: *mut NSError| {
                    let Some(image) = (unsafe { image.as_ref() }) else {
                        if !error.is_null() {
                            eprintln!("WKWebView visualization snapshot failed");
                        }
                        return;
                    };
                    let pasteboard = NSPasteboard::generalPasteboard();
                    let _ = pasteboard.clearContents();
                    let object: &ProtocolObject<dyn NSPasteboardWriting> =
                        ProtocolObject::from_ref(image);
                    let objects = NSArray::from_slice(&[object]);
                    if !pasteboard.writeObjects(&objects) {
                        eprintln!("failed to write visualization image to clipboard");
                    }
                });
            unsafe {
                webview.takeSnapshotWithConfiguration_completionHandler(None, &completion);
            }
            Ok(())
        }

        /// Capture the visible live visualization as PNG bytes without
        /// touching the user's clipboard. The snapshot completes
        /// asynchronously on WebKit's callback.
        pub fn capture_active_visualization_image<F>(&self, callback: F)
        where
            F: FnOnce(Result<Vec<u8>, String>) + 'static,
        {
            let Some(active) = self.active.as_ref() else {
                callback(Err("The visualization is not ready yet.".to_string()));
                return;
            };
            if !matches!(active.intent, WebPreviewIntent::Visualization { .. }) {
                callback(Err("There is no live visualization to capture.".to_string()));
                return;
            }
            let Some(webview) = active.webview.webkit() else {
                callback(Err(
                    "The visualization cannot be captured from this surface.".to_string(),
                ));
                return;
            };
            let callback = Rc::new(RefCell::new(Some(callback)));
            let completion_callback = callback.clone();
            let webview = webview.webview();
            let completion: RcBlock<dyn Fn(*mut NSImage, *mut NSError)> =
                RcBlock::new(move |image: *mut NSImage, error: *mut NSError| {
                    let result = match unsafe { image.as_ref() } {
                        Some(image) => visualization_png_bytes(image),
                        None if !error.is_null() => {
                            Err("The desktop could not capture the visualization.".to_string())
                        }
                        None => Err("The visualization is not ready yet.".to_string()),
                    };
                    if let Some(callback) = completion_callback.borrow_mut().take() {
                        callback(result);
                    }
                });
            unsafe {
                webview.takeSnapshotWithConfiguration_completionHandler(None, &completion);
            }
        }
    }

    fn visualization_png_bytes(image: &NSImage) -> Result<Vec<u8>, String> {
        let tiff = image
            .TIFFRepresentation()
            .ok_or_else(|| "The visualization snapshot had no image data.".to_string())?;
        let bitmap = NSBitmapImageRep::imageRepWithData(&tiff)
            .ok_or_else(|| "The visualization snapshot could not be encoded.".to_string())?;
        let properties = NSDictionary::<NSBitmapImageRepPropertyKey, AnyObject>::new();
        let png = unsafe {
            bitmap.representationUsingType_properties(NSBitmapImageFileType::PNG, &properties)
        }
        .ok_or_else(|| "The visualization snapshot could not be encoded as PNG.".to_string())?;
        Ok(png.to_vec())
    }

    fn to_rect(bounds: Bounds<Pixels>) -> Rect {
        let x = f32::from(bounds.origin.x) as f64;
        let y = f32::from(bounds.origin.y) as f64;
        let width = (f32::from(bounds.size.width) as f64).max(1.0);
        let height = (f32::from(bounds.size.height) as f64).max(1.0);
        Rect {
            position: LogicalPosition::new(x, y).into(),
            size: LogicalSize::new(width, height).into(),
        }
    }

    fn same_surface(left: &WebPreviewIntent, right: &WebPreviewIntent) -> bool {
        if left == right {
            return true;
        }
        match (left, right) {
            (WebPreviewIntent::StudioCanvas{session:left,..},WebPreviewIntent::StudioCanvas{session:right,..})=>left==right,
            (
                WebPreviewIntent::Studio { session: left, .. },
                WebPreviewIntent::Studio { session: right, .. },
            ) => left == right,
            (
                WebPreviewIntent::DocEditor {
                    path: left,
                    assets: left_assets,
                    ..
                },
                WebPreviewIntent::DocEditor {
                    path: right,
                    assets: right_assets,
                    ..
                },
            ) => {
                left == right
                    && reference_protocol_assets(left_assets)
                        .eq(reference_protocol_assets(right_assets))
            }
            (
                WebPreviewIntent::ProjectPreview {
                    project_id: left, ..
                },
                WebPreviewIntent::ProjectPreview {
                    project_id: right, ..
                },
            ) => left == right,
            _ => false,
        }
    }









    fn reference_protocol_assets(
        assets: &[super::DocEditorMention],
    ) -> impl Iterator<Item = (&str, &Path)> {
        assets.iter().filter_map(|asset| {
            let id = asset.target.strip_prefix("ref:design:")?;
            Some((id, asset.preview_path.as_deref()?))
        })
    }

    fn sync_active_doc_editor(active: &Active, next: &WebPreviewIntent) {
        let (
            WebPreviewIntent::DocEditor {
                document: current_document,
                files: current_files,
                assets: current_assets,
                theme: current_theme,
                ..
            },
            WebPreviewIntent::DocEditor {
                document,
                files,
                assets,
                theme,
                ..
            },
        ) = (&active.intent, next)
        else {
            return;
        };
        let mut script = String::new();
        if current_document != document {
            let Ok(document) = serde_json::to_string(document) else {
                return;
            };
            script.push_str(&format!("window.choroEditor?.loadDocument({document});"));
        }
        if current_files != files || current_assets != assets {
            let (Ok(files), Ok(assets)) =
                (serde_json::to_string(files), serde_json::to_string(assets))
            else {
                return;
            };
            script.push_str(&format!(
                "window.choroEditor?.setSources({files}, {assets});"
            ));
        }
        if current_theme != theme {
            let Ok(theme) = serde_json::to_string(theme) else {
                return;
            };
            script.push_str(&format!("window.choroEditor?.setTheme({theme});"));
        }
        if !script.is_empty() {
            let _ = active.webview.evaluate_script(&script);
        }
    }


    #[derive(Debug, PartialEq, Eq)]
    enum ProjectPreviewSyncAction<'a> {
        Navigate(&'a str),
        ReloadFromOrigin,
    }

    fn project_preview_sync_action<'a>(
        current: &WebPreviewIntent,
        next: &'a WebPreviewIntent,
    ) -> Option<ProjectPreviewSyncAction<'a>> {
        let (
            WebPreviewIntent::ProjectPreview {
                project_id: current_project,
                url: current_url,
                revision: current_revision,
            },
            WebPreviewIntent::ProjectPreview {
                project_id: next_project,
                url: next_url,
                revision: next_revision,
            },
        ) = (current, next)
        else {
            return None;
        };
        if current_project != next_project {
            return None;
        }
        if current_url != next_url {
            return Some(ProjectPreviewSyncAction::Navigate(next_url));
        }
        (current_revision != next_revision).then_some(ProjectPreviewSyncAction::ReloadFromOrigin)
    }

    fn sync_active_project_preview(active: &Active, next: &WebPreviewIntent) {
        match project_preview_sync_action(&active.intent, next) {
            Some(ProjectPreviewSyncAction::Navigate(url)) => {
                let _ = active.webview.load_url(url);
            }
            Some(ProjectPreviewSyncAction::ReloadFromOrigin) => {
                let _ = active.webview.reload_from_origin();
            }
            None => {}
        }
    }

    fn build(
        intent: &WebPreviewIntent,
        bounds: Bounds<Pixels>,
        window: &Window,
        messages: Rc<RefCell<VecDeque<DocEditorMessage>>>,
        preview_messages: Rc<RefCell<VecDeque<ProjectPreviewMessage>>>,
        project_preview_live_urls: Rc<RefCell<HashMap<ProjectId, String>>>,
        app: WebPreviewWake,
    ) -> Result<WebSurface, String> {
        let rect = to_rect(bounds);
        match intent {
            WebPreviewIntent::Url(url) => {
                let lease = WebKitLease::acquire()?;
                WebViewBuilder::new()
                    .with_url(url)
                    .with_bounds(rect)
                    .with_transparent(false)
                    .with_accept_first_mouse(true)
                    .build_as_child(window)
                    .map(|webview| WebSurface::WebKit(WebKitSurface::new(webview, lease)))
                    .map_err(|error| error.to_string())
            }
            WebPreviewIntent::ProjectPreview {
                project_id, url, ..
            } => {
                let lease = WebKitLease::acquire()?;
                let file_path = project_preview_file_path(url);
                let page_load_project = *project_id;
                let page_load_messages = preview_messages.clone();
                let page_load_live_urls = project_preview_live_urls;
                let page_load_app = app.clone();
                let webview = WebViewBuilder::new()
                    // Build on a neutral page so the guarded native message
                    // handler and isolated inspector are installed before any
                    // project script can run.
                    .with_url("about:blank")
                    .with_bounds(rect)
                    .with_transparent(false)
                    .with_accept_first_mouse(true)
                    .with_navigation_handler(|url| {
                        url.starts_with("http://")
                            || url.starts_with("https://")
                            || url.starts_with("file://")
                            || url == "about:blank"
                    })
                    .with_on_page_load_handler(move |event, url| {
                        page_load_live_urls
                            .borrow_mut()
                            .insert(page_load_project, url.clone());
                        enqueue_project_preview_message(
                            &page_load_messages,
                            &page_load_app,
                            ProjectPreviewMessage::AgentPageLoad {
                                project_id: page_load_project,
                                finished: matches!(event, PageLoadEvent::Finished),
                                url,
                            },
                        );
                    })
                    .build_as_child(window)
                    .map_err(|error| error.to_string())?;
                let Some(main_thread) = MainThreadMarker::new() else {
                    return Err("Preview must be created on the main thread.".to_string());
                };
                let controller = webview.manager();
                let inspector_world = project_preview_content_world(main_thread);
                let page_world = unsafe { WKContentWorld::pageWorld(main_thread) };
                ProjectPreviewMessageHandler::install(
                    &controller,
                    &inspector_world,
                    *project_id,
                    preview_messages.clone(),
                    app.clone(),
                    main_thread,
                );
                ProjectPreviewConsoleMessageHandler::install(
                    &controller,
                    &page_world,
                    *project_id,
                    preview_messages,
                    app,
                    main_thread,
                );
                if let Some(path) = file_path {
                    load_project_preview_file(&webview, &path);
                } else {
                    webview.load_url(url).map_err(|error| error.to_string())?;
                }
                Ok(WebSurface::WebKit(WebKitSurface::project_preview(
                    webview,
                    lease,
                    ProjectPreviewHandlers {
                        controller,
                        inspector_world,
                        page_world,
                    },
                )))
            }
            WebPreviewIntent::DocEditor {
                path,
                document,
                files,
                assets,
                theme,
            } => {
                let bootstrap = serde_json::json!({
                    "path": path,
                    "document": document,
                    "files": files,
                    "assets": assets,
                    "theme": theme,
                });
                let bootstrap =
                    serde_json::to_string(&bootstrap).unwrap_or_else(|_| "{}".to_string());
                let asset_root = Rc::new(RefCell::new(doc_asset_dir(path)));
                let protocol_asset_root = asset_root.clone();
                let reference_assets = Rc::new(
                    assets
                        .iter()
                        .filter_map(|asset| {
                            let id = asset.target.strip_prefix("ref:design:")?;
                            Some((id.to_string(), asset.preview_path.clone()?))
                        })
                        .collect::<HashMap<_, _>>(),
                );
                let protocol_reference_assets = reference_assets.clone();
                let ipc_messages = messages.clone();
                let ipc_app = app.clone();
                let lease = WebKitLease::acquire()?;
                WebViewBuilder::new()
                    .with_custom_protocol("choro-editor".into(), move |_, request| {
                        doc_editor_asset_response(request)
                    })
                    .with_custom_protocol("choro-asset".into(), move |_, request| {
                        doc_upload_asset_response(request, &protocol_asset_root.borrow())
                    })
                    .with_custom_protocol("choro-reference".into(), move |_, request| {
                        doc_reference_asset_response(request, &protocol_reference_assets)
                    })
                    .with_initialization_script(format!(
                        "window.__CHORO_BOOTSTRAP__ = {bootstrap};"
                    ))
                    .with_ipc_handler(move |request| {
                        if request.body().len() > MAX_DOC_EDITOR_MESSAGE_BYTES {
                            eprintln!("document editor message exceeded the size limit");
                            return;
                        }
                        match serde_json::from_str::<DocEditorMessage>(request.body()) {
                            Ok(message) => ipc_messages.borrow_mut().push_back(message),
                            Err(error) => eprintln!("invalid document editor message: {error}"),
                        }
                        let _ = ipc_app.refresh();
                    })
                    .with_url("choro-editor://localhost/index.html")
                    .with_bounds(rect)
                    // Keep WebKit's default white page hidden. GPUI already
                    // paints the active theme's background and loading state;
                    // the editor becomes visible after its `ready` message.
                    .with_visible(false)
                    .with_transparent(false)
                    .with_accept_first_mouse(true)
                    .with_navigation_handler(|url| {
                        url.starts_with("choro-editor://")
                            || url.starts_with("choro-asset://")
                            || url.starts_with("choro-reference://")
                    })
                    .build_as_child(window)
                    .map(|webview| WebSurface::WebKit(WebKitSurface::new(webview, lease)))
                    .map_err(|error| error.to_string())
            }
            WebPreviewIntent::StudioCanvas { session, document } => {
                let lease=WebKitLease::acquire()?;
                let session=*session;let html=document.clone();let ipc_app=app.clone();
                WebViewBuilder::new()
                    .with_custom_protocol("choro-canvas".into(),move |_,request|{
                        if request.uri().to_string()!="choro-canvas://localhost/index.html" {return Response::builder().status(404).body(Cow::Owned(Vec::new())).unwrap();}
                        Response::builder().header("Content-Type","text/html; charset=utf-8").body(Cow::Owned(html.as_bytes().to_vec())).unwrap()
                    })
                    .with_custom_protocol("choro-canvas-image".into(),move |_,request|{
                        let bytes=(||{
                            if request.method().as_str()!="GET"||request.uri().host()!=Some("localhost")||request.uri().query().is_some(){return None;}
                            let path=request.uri().path().strip_prefix('/')?;let (owner,key)=path.split_once('/')?;
                            let owner=owner.parse::<Uuid>().ok()?;if owner!=session{return None;}
                            let key=key.strip_suffix(".png")?.parse::<Uuid>().ok()?;
                            super::super::studio_canvas::image_bytes(owner,key)
                        })();
                        match bytes{Some(bytes)=>Response::builder().header("Content-Type","image/png").header("Cache-Control","no-store").body(Cow::Owned((*bytes).clone())).unwrap(),None=>Response::builder().status(404).body(Cow::Owned(Vec::new())).unwrap()}
                    })
                    .with_ipc_handler(move|request|{
                        if request.uri().to_string()=="choro-canvas://localhost/index.html" && (super::super::studio_canvas::enqueue(request.body(),session) || super::super::studio_editor::enqueue_inline(session,request.body())) {let _=ipc_app.studio_refresh();}
                    })
                    .with_url("choro-canvas://localhost/index.html")
                    .with_bounds(rect).with_transparent(false).with_accept_first_mouse(true)
                    .with_navigation_handler(move |url|url=="choro-canvas://localhost/index.html" || url=="about:srcdoc" || url=="about:blank")
                    .build_as_child(window).map(|webview|WebSurface::WebKit(WebKitSurface::with_keyboard_routing(webview,lease,true))).map_err(|e|e.to_string())
            }
            WebPreviewIntent::Studio { session, document } => {
                let lease = WebKitLease::acquire()?;
                let html = document.clone();
                let session = *session;
                let ipc_app = app.clone();
                WebViewBuilder::new()
                    .with_custom_protocol("choro-studio".into(), move |_, request| {
                        if request.uri().path() != "/index.html" {
                            return Response::builder().status(404).body(Cow::Owned(Vec::new())).unwrap();
                        }
                        Response::builder().header("Content-Type", "text/html; charset=utf-8").body(Cow::Owned(html.as_bytes().to_vec())).unwrap()
                    })
                    .with_ipc_handler(move |request| {
                        // The sandboxed design frame must never reach native mutations.
                        if request.uri().to_string() != "choro-studio://localhost/index.html" { return; }
                        if request.body().len() > 12 * 1024 * 1024 {
                            super::super::studio_editor::enqueue(serde_json::json!({"session":session,"type":"render-error","error":"This edit is too large to save. Your editing buffer is still open; reduce its size and retry."}));
                            let _=ipc_app.studio_refresh();return;
                        }
                        if let Ok(value) = serde_json::from_str::<serde_json::Value>(request.body()) {
                            if value.get("session").and_then(|v|v.as_str()).and_then(|s|s.parse::<Uuid>().ok()) == Some(session) {
                                super::super::studio_editor::enqueue(value);
                                let _ = ipc_app.studio_refresh();
                            }
                        }
                    })
                    .with_url("choro-studio://localhost/index.html")
                    .with_bounds(rect).with_transparent(false).with_accept_first_mouse(true)
                    .with_navigation_handler(|url| url == "choro-studio://localhost/index.html" || url == "about:srcdoc" || url == "about:blank")
                    .build_as_child(window)
                    .map(|webview|WebSurface::WebKit(WebKitSurface::with_keyboard_routing(webview,lease,true)))
                    .map_err(|error|error.to_string())
            }
            WebPreviewIntent::Visualization { path, theme, .. } => {
                let html =
                    visualization_document(path, theme).map_err(|error| error.to_string())?;
                let lease = WebKitLease::acquire()?;
                WebViewBuilder::new()
                    .with_html(html)
                    .with_bounds(rect)
                    .with_transparent(false)
                    .with_navigation_handler(|url| {
                        url == "about:blank" || url.starts_with("data:text/html")
                    })
                    .build_as_child(window)
                    .map(|webview| WebSurface::WebKit(WebKitSurface::new(webview, lease)))
                    .map_err(|error| error.to_string())
            }
        }
    }

    fn project_preview_file_path(value: &str) -> Option<PathBuf> {
        let url = url::Url::parse(value).ok()?;
        (url.scheme() == "file")
            .then(|| url.to_file_path().ok())
            .flatten()
    }

    fn load_project_preview_file(webview: &WebView, path: &Path) {
        let Some(read_access_root) = path.parent() else {
            return;
        };
        let path = NSString::from_str(&path.to_string_lossy());
        let read_access_root = NSString::from_str(&read_access_root.to_string_lossy());
        unsafe {
            let file_url = NSURL::fileURLWithPath(&path);
            let read_access_url = NSURL::fileURLWithPath(&read_access_root);
            webview
                .webview()
                .as_super()
                .loadFileURL_allowingReadAccessToURL(&file_url, &read_access_url);
        }
    }

    fn doc_editor_asset_response(_request: Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
        // Keep the editor in one WebKit response. WKWebView can apply stricter
        // custom-scheme/CSP rules to subresources than to the main document,
        // which previously left a correctly colored but empty, non-editable
        // surface when the JavaScript bundle was rejected.
        let css = String::from_utf8_lossy(DOC_EDITOR_CSS);
        let javascript = String::from_utf8_lossy(DOC_EDITOR_JS);
        let html = format!(
            r#"<!doctype html>
<html>
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src choro-asset: choro-reference: data: blob:; font-src data:; media-src choro-asset: choro-reference: data: blob:">
  <style>{css}</style>
</head>
<body>
  <div id="root"><div class="choro-boot-status">Loading document editor…</div></div>
  <script>
    window.addEventListener("error", function (event) {{
      var root = document.getElementById("root");
      if (root) root.innerHTML = '<div class="choro-boot-error"><strong>Could not start the document editor</strong><span></span></div>';
      var detail = event && event.message ? String(event.message) : "Unknown editor error";
      var span = root && root.querySelector("span");
      if (span) span.textContent = detail;
      try {{ window.ipc && window.ipc.postMessage(JSON.stringify({{ type: "error", message: detail }})); }} catch (_) {{}}
    }});
  </script>
  <script>{javascript}</script>
</body>
</html>"#
        );
        Response::builder()
            .header(CONTENT_TYPE, "text/html; charset=utf-8")
            .body(Cow::Owned(html.into_bytes()))
            .expect("valid editor HTML response")
    }

    fn doc_upload_asset_response(
        request: Request<Vec<u8>>,
        asset_root: &Path,
    ) -> Response<Cow<'static, [u8]>> {
        let name = request.uri().path().trim_start_matches('/');
        if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
            return Response::builder()
                .status(404)
                .body(Cow::Borrowed(&b"not found"[..]))
                .expect("valid not-found response");
        }
        match fs::read(asset_root.join(name)) {
            Ok(bytes) => Response::builder()
                .header(CONTENT_TYPE, mime_for_name(name))
                .body(Cow::Owned(bytes))
                .expect("valid document asset response"),
            Err(_) => Response::builder()
                .status(404)
                .body(Cow::Borrowed(&b"not found"[..]))
                .expect("valid not-found response"),
        }
    }

    fn doc_reference_asset_response(
        request: Request<Vec<u8>>,
        assets: &HashMap<String, PathBuf>,
    ) -> Response<Cow<'static, [u8]>> {
        let id = request.uri().path().trim_start_matches('/');
        let Some(path) = assets.get(id) else {
            return Response::builder()
                .status(404)
                .body(Cow::Borrowed(&b"not found"[..]))
                .expect("valid not-found response");
        };
        match fs::read(path) {
            Ok(bytes) => Response::builder()
                .header(
                    CONTENT_TYPE,
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .map(mime_for_name)
                        .unwrap_or("application/octet-stream"),
                )
                .body(Cow::Owned(bytes))
                .expect("valid reference asset response"),
            Err(_) => Response::builder()
                .status(404)
                .body(Cow::Borrowed(&b"not found"[..]))
                .expect("valid not-found response"),
        }
    }

    fn doc_asset_dir(path: &Path) -> PathBuf {
        path.with_extension("assets")
    }

    fn persist_doc_upload(
        document_path: &Path,
        original_name: &str,
        mime: &str,
        data_url: &str,
    ) -> Result<String, String> {
        let (_, encoded) = data_url
            .split_once(',')
            .ok_or_else(|| "The uploaded file had invalid data.".to_string())?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| "The uploaded file could not be decoded.".to_string())?;
        if bytes.len() > MAX_DOC_UPLOAD_BYTES {
            return Err("Files larger than 25 MB are not supported.".to_string());
        }
        let extension = safe_upload_extension(original_name, mime);
        let file_name = format!("{}.{}", uuid::Uuid::new_v4().simple(), extension);
        let asset_dir = doc_asset_dir(document_path);
        fs::create_dir_all(&asset_dir)
            .map_err(|error| format!("Could not create the document assets folder: {error}"))?;
        let path = asset_dir.join(&file_name);
        fs::write(&path, bytes)
            .map_err(|error| format!("Could not save the uploaded file: {error}"))?;
        Ok(format!("choro-asset://localhost/{file_name}"))
    }

    fn safe_upload_extension(name: &str, mime: &str) -> &'static str {
        match mime {
            "image/png" => "png",
            "image/jpeg" => "jpg",
            "image/gif" => "gif",
            "image/webp" => "webp",
            "image/svg+xml" => "svg",
            "video/mp4" => "mp4",
            "audio/mpeg" => "mp3",
            "application/pdf" => "pdf",
            _ => match Path::new(name)
                .extension()
                .and_then(|extension| extension.to_str())
                .map(str::to_ascii_lowercase)
                .as_deref()
            {
                Some("png") => "png",
                Some("jpg" | "jpeg") => "jpg",
                Some("gif") => "gif",
                Some("webp") => "webp",
                Some("svg") => "svg",
                Some("mp4") => "mp4",
                Some("mp3") => "mp3",
                Some("pdf") => "pdf",
                _ => "bin",
            },
        }
    }

    fn mime_for_name(name: &str) -> &'static str {
        match Path::new(name)
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("png") => "image/png",
            Some("jpg" | "jpeg") => "image/jpeg",
            Some("gif") => "image/gif",
            Some("webp") => "image/webp",
            Some("svg") => "image/svg+xml",
            Some("mp4") => "video/mp4",
            Some("mp3") => "audio/mpeg",
            Some("pdf") => "application/pdf",
            _ => "application/octet-stream",
        }
    }

    fn visualization_document(
        path: &std::path::Path,
        theme: &VisualizationTheme,
    ) -> io::Result<String> {
        let metadata = fs::metadata(path)?;
        if metadata.len() > MAX_VISUALIZATION_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "visualization exceeds the 2 MB limit",
            ));
        }
        let source = fs::read_to_string(path)?;
        let chrome = visualization_chrome(theme);
        Ok(inject_visualization_chrome(&source, &chrome))
    }

    fn visualization_chrome(theme: &VisualizationTheme) -> String {
        format!(
            r#"<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src data: blob:; style-src 'unsafe-inline' https://cdnjs.cloudflare.com https://cdn.jsdelivr.net https://unpkg.com https://fonts.googleapis.com https://fonts.bunny.net; font-src data: https://fonts.gstatic.com https://fonts.bunny.net; script-src 'unsafe-inline' https://cdnjs.cloudflare.com https://esm.sh https://cdn.jsdelivr.net https://unpkg.com; connect-src 'none'; media-src data: blob:">
<style>
:root {{
  color-scheme: light dark;
  --background: {background}; --foreground: {foreground};
  --card: {card}; --card-foreground: {card_foreground};
  --muted: {muted}; --muted-foreground: {muted_foreground};
  --border: {border}; --input: {border}; --ring: {primary};
  --primary: {primary}; --primary-foreground: {primary_foreground};
  --secondary: {muted}; --secondary-foreground: {foreground};
  --accent: {accent}; --accent-foreground: {accent_foreground};
  --destructive: #d65d67;
  --viz-series-1: {primary}; --viz-series-2: #62a0d2; --viz-series-3: #72b09a;
  --viz-series-4: #d0a45f; --viz-series-5: #b787c4; --viz-series-6: #cf7f83;
  --font-size-base: 14px;
}}
* {{ box-sizing: border-box; }}
html, body {{ margin: 0; min-width: 0; background: var(--background); color: var(--foreground); }}
body {{ padding: 16px; font: 400 var(--font-size-base)/1.45 -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; }}
button, input, select, textarea {{ font: inherit; color: inherit; }}
.card {{ background: var(--card); color: var(--card-foreground); border: 1px solid var(--border); border-radius: 10px; padding: 12px; }}
.viz-grid {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(150px, 1fr)); gap: 10px; }}
.viz-row, .viz-controls {{ display: flex; align-items: center; flex-wrap: wrap; gap: 8px; }}
.viz-controls {{ margin-bottom: 12px; }}
.viz-stat-value {{ font-size: 1.45em; font-weight: 500; }}
.viz-badge {{ display: inline-flex; padding: 2px 8px; border-radius: 999px; background: var(--accent); color: var(--accent-foreground); }}
.text-small {{ font-size: 0.86em; }} .text-muted {{ color: var(--muted-foreground); }}
.sr-only {{ position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip: rect(0,0,0,0); white-space: nowrap; border: 0; }}
.btn {{ appearance: none; border: 1px solid var(--border); border-radius: 7px; padding: 6px 10px; background: var(--card); cursor: pointer; }}
.btn:hover {{ background: var(--accent); }} .btn-primary {{ background: var(--primary); color: var(--primary-foreground); }}
.btn-ghost {{ border-color: transparent; background: transparent; }} .btn-block {{ width: 100%; }}
.form-label {{ display: grid; gap: 5px; }} .form-control, .form-select {{ width: 100%; border: 1px solid var(--input); border-radius: 7px; padding: 6px 8px; background: var(--card); }}
.form-range {{ width: 100%; }}
a {{ color: var(--foreground); }} svg, canvas {{ max-width: 100%; }}
</style>"#,
            background = theme.background,
            foreground = theme.foreground,
            card = theme.card,
            card_foreground = theme.card_foreground,
            muted = theme.muted,
            muted_foreground = theme.muted_foreground,
            border = theme.border,
            primary = theme.primary,
            primary_foreground = theme.primary_foreground,
            accent = theme.accent,
            accent_foreground = theme.accent_foreground,
        )
    }

    fn inject_visualization_chrome(source: &str, chrome: &str) -> String {
        let lowercase = source.to_ascii_lowercase();
        if let Some(head_start) = lowercase.find("<head") {
            if let Some(relative_end) = source[head_start..].find('>') {
                let insert_at = head_start + relative_end + 1;
                let mut document = String::with_capacity(source.len() + chrome.len());
                document.push_str(&source[..insert_at]);
                document.push_str(chrome);
                document.push_str(&source[insert_at..]);
                return document;
            }
        }
        if let Some(html_start) = lowercase.find("<html") {
            if let Some(relative_end) = source[html_start..].find('>') {
                let insert_at = html_start + relative_end + 1;
                let mut document = String::with_capacity(source.len() + chrome.len() + 13);
                document.push_str(&source[..insert_at]);
                document.push_str("<head>");
                document.push_str(chrome);
                document.push_str("</head>");
                document.push_str(&source[insert_at..]);
                return document;
            }
        }
        format!("<!doctype html><html><head>{chrome}</head><body>{source}</body></html>")
    }

    fn surface_visible(suspended: bool, content_ready: bool) -> bool {
        !suspended && content_ready
    }

    pub fn restore_focus(window: &Window) {
        use objc::runtime::Object;
        use objc::{msg_send, sel, sel_impl};
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};

        let Ok(handle) = HasWindowHandle::window_handle(window) else {
            return;
        };
        let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
            return;
        };
        let ns_view = appkit.ns_view.as_ptr() as *mut Object;
        if ns_view.is_null() {
            return;
        }
        unsafe {
            let ns_window: *mut Object = msg_send![ns_view, window];
            if !ns_window.is_null() {
                let _: bool = msg_send![ns_window, makeFirstResponder: ns_view];
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use ide_core::project::ProjectId;
        use uuid::Uuid;

        #[test]
        fn preview_keyboard_routes_text_and_editing_without_consuming_app_shortcuts() {
            use super::{is_preview_editing_key, preview_edit_action};
            use objc2::sel;
            use objc2_app_kit::NSEventModifierFlags as Flags;

            for key in ["a", "A", " ", "\r", "\t", "\u{7f}", "\u{f702}", "é", "א", ""] {
                assert!(is_preview_editing_key(key, Flags::empty()), "{key:?}");
                assert!(is_preview_editing_key(key, Flags::Shift), "{key:?}");
                assert!(is_preview_editing_key(key, Flags::Option), "{key:?}");
            }
            for key in ["a", "c", "x", "v", "z", "s", "\r", "\u{f702}", "\u{7f}"] {
                assert!(is_preview_editing_key(key, Flags::Command), "{key:?}");
                assert!(is_preview_editing_key(key, Flags::Command | Flags::Shift), "{key:?}");
            }
            assert!(is_preview_editing_key("\r", Flags::Control));
            for key in ["q", "w", "m", "h", "`"] {
                assert!(!is_preview_editing_key(key, Flags::Command), "{key:?}");
            }
            assert_eq!(preview_edit_action("v", Flags::Command, false), Some(sel!(paste:)));
            assert_eq!(preview_edit_action("a", Flags::Command, false), Some(sel!(selectAll:)));
            assert_eq!(preview_edit_action("z", Flags::Command, false), Some(sel!(undo:)));
            assert_eq!(preview_edit_action("z", Flags::Command | Flags::Shift, false), Some(sel!(redo:)));
            assert!(preview_edit_action("v", Flags::empty(), false).is_none());
            assert!(preview_edit_action("v", Flags::Command | Flags::Shift, false).is_none());
            for key in ["z", "Z", "y"] {
                for flags in [Flags::Command, Flags::Control, Flags::Command | Flags::Shift, Flags::Control | Flags::Shift] {
                    assert!(is_preview_editing_key(key, flags));
                    assert!(preview_edit_action(key, flags, true).is_none(), "Studio history must reach DOM keyDown");
                }
            }
            assert_eq!(preview_edit_action("v", Flags::Command, true), Some(sel!(paste:)));
        }

        #[test]
        fn overlapping_native_overlays_only_reveal_after_last_dismissal() {
            let mut overlays = super::super::NativeOverlays::default();
            overlays.begin_popup();
            overlays.begin_popup();
            overlays.end_popup();
            assert!(overlays.hidden());
            overlays.modal = true;
            overlays.end_popup();
            assert!(
                overlays.hidden(),
                "closing Rename's menu must not reveal beneath its dialog"
            );
            overlays.modal = false;
            assert!(!overlays.hidden());
            overlays.end_popup();
            assert!(
                !overlays.hidden(),
                "a late release must not underflow the count"
            );
        }

        use super::{
            bind_project_preview_message, decode_project_preview_console_entry,
            decode_project_preview_message, inject_visualization_chrome,
            preview_key_event_data, project_preview_message_live_url, project_preview_sync_action,
            same_surface, surface_visible, ProjectPreviewSyncAction, WebKitLease,
            MAX_PROJECT_PREVIEW_CONSOLE_MESSAGE_BYTES, MAX_PROJECT_PREVIEW_MESSAGE_BYTES,
        };

        #[test]
        fn project_preview_navigation_events_supply_the_live_url_cache() {
            let project_id = ProjectId(Uuid::from_u128(7));
            let message = ProjectPreviewMessage::AgentPageLoad {
                project_id,
                finished: false,
                url: "http://127.0.0.1:5173/ready".into(),
            };

            assert_eq!(
                project_preview_message_live_url(&message),
                Some((project_id, "http://127.0.0.1:5173/ready"))
            );
        }
        use crate::ui::center::web_preview::{
            ProjectPreviewConsoleLevel, ProjectPreviewMessage, WebPreviewIntent,
        };


        #[test]
        fn web_surface_stays_hidden_until_content_is_ready() {
            assert!(!surface_visible(false, false));
            assert!(surface_visible(false, true));
            assert!(!surface_visible(true, true));
        }



        #[test]
        fn wraps_fragment_in_document() {
            let output = inject_visualization_chrome("<div>Chart</div>", "<style>x</style>");
            assert!(output.contains("<head><style>x</style></head>"));
            assert!(output.contains("<body><div>Chart</div></body>"));
        }

        #[test]
        fn injects_into_existing_head() {
            let output = inject_visualization_chrome(
                "<!doctype html><html><head><title>V</title></head><body /></html>",
                "<style>x</style>",
            );
            assert!(output.contains("<head><style>x</style><title>V</title>"));
        }

        #[test]
        fn inspector_messages_are_bound_to_their_source_project() {
            let project_id = ProjectId(Uuid::from_u128(1));
            let message = decode_project_preview_message(
                r#"{"kind":"submitReview","comment":"Move this","targetKind":"area","element":null,"area":null}"#,
            )
            .expect("valid inspector message");

            match bind_project_preview_message(project_id, message) {
                ProjectPreviewMessage::SubmitReview {
                    project_id: bound_project,
                    comment,
                    target_kind,
                    ..
                } => {
                    assert_eq!(bound_project, project_id);
                    assert_eq!(comment, "Move this");
                    assert_eq!(target_kind, "area");
                }
                other => panic!("unexpected bound message: {other:?}"),
            }
        }

        #[test]
        fn preview_focus_shortcut_is_bound_to_its_source_project() {
            let project_id = ProjectId(Uuid::from_u128(2));
            let message = decode_project_preview_message(r#"{"kind":"toggleFocusMode"}"#)
                .expect("valid focus-mode shortcut");

            assert!(matches!(
                bind_project_preview_message(project_id, message),
                ProjectPreviewMessage::ToggleFocusMode {
                    project_id: bound_project
                } if bound_project == project_id
            ));
        }

        #[test]
        fn inspector_cannot_forge_native_capture_completion() {
            let forged = r#"{"kind":"captureReady","agentId":"00000000-0000-0000-0000-000000000001","imageBase64":"AA=="}"#;
            assert!(decode_project_preview_message(forged).is_err());
        }

        #[test]
        fn console_bridge_accepts_only_bounded_typed_entries() {
            let entry = decode_project_preview_console_entry(
                r#"{"level":"error","message":"Boom","source":"http://localhost/app.js","line":7,"column":11}"#,
            )
            .expect("valid console entry");

            assert_eq!(entry.level, ProjectPreviewConsoleLevel::Error);
            assert_eq!(entry.message, "Boom");
            assert_eq!(entry.line, Some(7));
            assert!(decode_project_preview_console_entry(
                r#"{"level":"fatal","message":"Nope","source":null,"line":null,"column":null}"#,
            )
            .is_err());
            let oversized = "x".repeat(MAX_PROJECT_PREVIEW_CONSOLE_MESSAGE_BYTES + 1);
            assert!(decode_project_preview_console_entry(&oversized).is_err());
        }

        #[test]
        fn agent_action_results_are_bound_to_their_preview_project() {
            let project_id = ProjectId(Uuid::from_u128(7));
            let command_id = Uuid::from_u128(9);
            let message = decode_project_preview_message(&format!(
                r#"{{"kind":"agentActionResult","commandId":"{command_id}","success":true,"capture":false,"result":{{"ok":true}}}}"#
            ))
            .expect("valid agent action result");

            match bind_project_preview_message(project_id, message) {
                ProjectPreviewMessage::AgentActionResult {
                    project_id: bound_project,
                    command_id: bound_command,
                    success,
                    ..
                } => {
                    assert_eq!(bound_project, project_id);
                    assert_eq!(bound_command, command_id);
                    assert!(success);
                }
                other => panic!("unexpected bound message: {other:?}"),
            }
        }

        #[test]
        fn navigation_readiness_is_a_distinct_host_acknowledgement() {
            let project_id = ProjectId(Uuid::from_u128(7));
            let command_id = Uuid::from_u128(11);
            let message = decode_project_preview_message(&format!(
                r#"{{"kind":"agentNavigationReady","commandId":"{command_id}","result":{{"ok":true,"target":"About"}}}}"#
            ))
            .expect("valid navigation readiness message");

            match bind_project_preview_message(project_id, message) {
                ProjectPreviewMessage::AgentNavigationReady {
                    project_id: bound_project,
                    command_id: bound_command,
                    result_json,
                } => {
                    assert_eq!(bound_project, project_id);
                    assert_eq!(bound_command, command_id);
                    assert!(result_json.contains("\"target\":\"About\""));
                }
                other => panic!("unexpected bound message: {other:?}"),
            }
        }

        #[test]
        fn same_document_navigation_settlement_keeps_its_command_identity() {
            let project_id = ProjectId(Uuid::from_u128(7));
            let command_id = Uuid::from_u128(12);
            let message = decode_project_preview_message(&format!(
                r#"{{"kind":"agentNavigationSettled","commandId":"{command_id}","url":"file:///tmp/index.html#about"}}"#
            ))
            .expect("valid navigation settlement message");

            match bind_project_preview_message(project_id, message) {
                ProjectPreviewMessage::AgentNavigationSettled {
                    project_id: bound_project,
                    command_id: bound_command,
                    url,
                } => {
                    assert_eq!(bound_project, project_id);
                    assert_eq!(bound_command, command_id);
                    assert!(url.ends_with("#about"));
                }
                other => panic!("unexpected bound message: {other:?}"),
            }
        }

        #[test]
        fn native_input_requests_are_bound_and_decode_modifiers() {
            let project_id = ProjectId(Uuid::from_u128(7));
            let command_id = Uuid::from_u128(10);
            let message = decode_project_preview_message(&format!(
                r#"{{"kind":"agentNativeInputRequest","commandId":"{command_id}","action":"key","key":"Enter","code":"Enter","meta":true}}"#
            ))
            .expect("valid native input request");

            match bind_project_preview_message(project_id, message) {
                ProjectPreviewMessage::AgentNativeInputRequest {
                    project_id: bound_project,
                    command_id: bound_command,
                    action,
                    key,
                    meta,
                    ..
                } => {
                    assert_eq!(bound_project, project_id);
                    assert_eq!(bound_command, command_id);
                    assert_eq!(action, "key");
                    assert_eq!(key.as_deref(), Some("Enter"));
                    assert!(meta);
                }
                other => panic!("unexpected bound message: {other:?}"),
            }
        }

        #[test]
        fn native_key_mapping_covers_navigation_and_character_codes() {
            assert_eq!(
                preview_key_event_data("Enter", Some("Enter")),
                ("\r".into(), 36)
            );
            assert_eq!(
                preview_key_event_data("ArrowLeft", Some("ArrowLeft")),
                ("\u{f702}".into(), 123)
            );
            assert_eq!(preview_key_event_data("a", Some("KeyA")), ("a".into(), 0));
        }

        #[test]
        fn oversized_inspector_messages_are_rejected() {
            let body = "x".repeat(MAX_PROJECT_PREVIEW_MESSAGE_BYTES + 1);
            assert!(decode_project_preview_message(&body).is_err());
        }

        #[test]
        fn project_previews_never_reuse_another_projects_webview() {
            let url = "http://127.0.0.1:4173".to_string();
            let first = WebPreviewIntent::ProjectPreview {
                project_id: ProjectId(Uuid::from_u128(1)),
                url: url.clone(),
                revision: 1,
            };
            let second = WebPreviewIntent::ProjectPreview {
                project_id: ProjectId(Uuid::from_u128(2)),
                url,
                revision: 1,
            };

            assert!(!same_surface(&first, &second));
        }

        #[test]
        fn project_preview_revision_reloads_the_existing_webview() {
            let project_id = ProjectId(Uuid::from_u128(1));
            let first = WebPreviewIntent::ProjectPreview {
                project_id,
                url: "http://127.0.0.1:4173/dashboard".into(),
                revision: 1,
            };
            let refreshed = WebPreviewIntent::ProjectPreview {
                project_id,
                url: "http://127.0.0.1:4173/dashboard".into(),
                revision: 2,
            };

            assert!(same_surface(&first, &refreshed));
            assert_eq!(
                project_preview_sync_action(&first, &refreshed),
                Some(ProjectPreviewSyncAction::ReloadFromOrigin)
            );
        }

        #[test]
        fn project_preview_navigation_reuses_the_projects_webview() {
            let project_id = ProjectId(Uuid::from_u128(1));
            let first = WebPreviewIntent::ProjectPreview {
                project_id,
                url: "http://127.0.0.1:4173/dashboard".into(),
                revision: 1,
            };
            let navigated = WebPreviewIntent::ProjectPreview {
                project_id,
                url: "http://127.0.0.1:4173/settings".into(),
                revision: 1,
            };

            assert!(same_surface(&first, &navigated));
            assert_eq!(
                project_preview_sync_action(&first, &navigated),
                Some(ProjectPreviewSyncAction::Navigate(
                    "http://127.0.0.1:4173/settings"
                ))
            );
        }

        #[test]
        fn webkit_lease_allows_only_one_native_surface() {
            let first = WebKitLease::acquire().expect("first WebKit surface should acquire");
            assert!(WebKitLease::acquire().is_err());

            drop(first);
            let replacement =
                WebKitLease::acquire().expect("replacement should acquire after teardown");
            drop(replacement);
        }










    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use std::path::Path;

    use gpui::{Bounds, Pixels, Window};
    use super::WebPreviewWake;

    use super::{
        DocEditorMessage, ProjectPreviewMessage, WebPreviewIntent,
    };

    pub struct WebPreviewHost;

    impl WebPreviewHost {
        pub fn new(_app: WebPreviewWake) -> Self {
            Self
        }
        pub fn set_intent(&mut self, _intent: Option<WebPreviewIntent>) -> bool {
            false
        }
        pub fn set_suspended(&mut self, _suspended: bool) {}
        pub fn set_overlay_suspended(&mut self, _suspended: bool) {}
        pub fn set_modal_suspended(&mut self, _suspended: bool) {}
        pub fn begin_popup(&mut self) {}
        pub fn end_popup(&mut self) {}
        pub fn is_route_suspended(&self) -> bool {
            false
        }
        pub fn begin_transcript_layout(&mut self) {}
        pub fn finish_transcript_layout(&mut self) {}
        pub fn place(&mut self, _bounds: Bounds<Pixels>, _window: &Window) {}
        pub fn take_doc_editor_messages(&mut self) -> Vec<DocEditorMessage> {
            Vec::new()
        }
        pub fn take_project_preview_messages(&mut self) -> Vec<ProjectPreviewMessage> {
            Vec::new()
        }
        pub fn set_project_preview_inspecting(&self, _inspecting: bool) -> Result<(), String> {
            Err("Project Preview is only available on macOS.".to_string())
        }
        pub fn reload_project_preview(&self) -> Result<(), String> {
            Err("Project Preview is only available on macOS.".to_string())
        }
        pub fn navigate_project_preview_url(
            &self,
            _project_id: ide_core::project::ProjectId,
            _url: &str,
        ) -> Result<(), String> {
            Err("Project Preview is only available on macOS.".to_string())
        }
        pub fn navigate_project_preview_history(&self, _forward: bool) -> Result<(), String> {
            Err("Project Preview is only available on macOS.".to_string())
        }
        pub fn project_preview_live_url(
            &self,
            _project_id: ide_core::project::ProjectId,
        ) -> Result<String, String> {
            Err("Project Preview is only available on macOS.".to_string())
        }
        pub fn cached_project_preview_live_url(
            &self,
            _project_id: ide_core::project::ProjectId,
        ) -> Option<String> {
            None
        }
        pub fn is_project_preview_active(&self, _project_id: ide_core::project::ProjectId) -> bool {
            false
        }
        pub fn execute_project_preview_agent_command(
            &self,
            _project_id: ide_core::project::ProjectId,
            _command_id: uuid::Uuid,
            _action: &str,
            _payload_json: &str,
            _policy_json: &str,
        ) -> Result<(), String> {
            Err("Project Preview is only available on macOS.".to_string())
        }
        pub fn activate_project_preview_navigation(
            &self,
            _project_id: ide_core::project::ProjectId,
            _command_id: uuid::Uuid,
        ) -> Result<(), String> {
            Err("Project Preview is only available on macOS.".to_string())
        }
        pub fn cancel_project_preview_agent_command(
            &self,
            _project_id: ide_core::project::ProjectId,
            _command_id: uuid::Uuid,
            _reason: &str,
        ) -> Result<(), String> {
            Ok(())
        }
        #[allow(clippy::too_many_arguments)]
        pub fn perform_project_preview_native_input(
            &self,
            _project_id: ide_core::project::ProjectId,
            _command_id: uuid::Uuid,
            _action: &str,
            _x: Option<f64>,
            _y: Option<f64>,
            _key: Option<&str>,
            _code: Option<&str>,
            _meta: bool,
            _control: bool,
            _alt: bool,
            _shift: bool,
        ) -> Result<(), String> {
            Err("Project Preview is only available on macOS.".to_string())
        }
        pub fn capture_project_preview_agent_snapshot(
            &self,
            _project_id: ide_core::project::ProjectId,
            _command_id: uuid::Uuid,
            _result_json: String,
        ) {
        }
        #[allow(clippy::too_many_arguments)]
        pub fn capture_project_preview_review(
            &self,
            _project_id: ide_core::project::ProjectId,
            _agent_id: uuid::Uuid,
            _url: String,
            _comment: String,
            _target_kind: String,
            _element: Option<ide_core::visual_review::VisualElementSelection>,
            _area: Option<ide_core::visual_review::VisualAreaSelection>,
        ) {
        }
        pub fn finish_project_preview_submission(
            &self,
            _project_id: ide_core::project::ProjectId,
            _succeeded: bool,
        ) {
        }
        pub fn doc_editor_ready_for(&self, _path: &Path) -> bool {
            false
        }
        pub fn studio_reply(&self, _value: &serde_json::Value) {}
        pub fn canvas_reply(&self, _value: &serde_json::Value) {}
        pub fn copy_active_visualization_image(&self) -> Result<(), String> {
            Err("Copy image is only available on macOS.".to_string())
        }
        pub fn capture_active_visualization_image<F>(&self, callback: F)
        where
            F: FnOnce(Result<Vec<u8>, String>) + 'static,
        {
            callback(Err(
                "Visualization capture is only available on macOS.".to_string()
            ));
        }
    }

    pub fn restore_focus(_window: &Window) {}
}

pub use imp::{restore_focus, WebPreviewHost};
