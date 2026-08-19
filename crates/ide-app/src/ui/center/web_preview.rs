//! Single native web surface shared by design previews and chat visualizations.
//!
//! GPUI paints the application into one GPU surface. `WKWebView` is therefore a
//! native child layered above a rectangle reserved by GPUI. Keeping exactly one
//! live child avoids focus, clipping, and z-order problems when chat rows scroll
//! or dialogs open.

use std::path::PathBuf;

use gpui::{App, AsyncApp};
use gpui_component::ActiveTheme;
use ide_core::project::ProjectId;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::state::docs::ChoroDocument;

const MAX_VISUALIZATION_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WebPreviewIntent {
    Url(String),
    PenpotUrl {
        url: String,
        theme: PenpotTheme,
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

/// The Choro design-system colors applied to the embedded Design editor's UI.
/// These tokens style editor chrome only; artwork on the canvas remains owned
/// by the design file and is never recolored by an application theme change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PenpotTheme {
    name: String,
    dark: bool,
    sink: String,
    nav: String,
    base: String,
    surface: String,
    surface_2: String,
    focus: String,
    text_1: String,
    text_2: String,
    text_3: String,
    text_4: String,
    line: String,
    line_2: String,
    accent: String,
    accent_2: String,
    on_accent: String,
    accent_soft: String,
    accent_line: String,
    info: String,
    overlay: String,
    shadow: String,
}

impl PenpotTheme {
    pub fn from_app(cx: &App) -> Self {
        use crate::ui::design;

        Self {
            name: cx.theme().theme_name().to_string(),
            dark: cx.theme().mode.is_dark(),
            sink: design::sink(cx).to_string(),
            nav: design::nav(cx).to_string(),
            base: design::base(cx).to_string(),
            surface: design::surface(cx).to_string(),
            surface_2: design::surface_2(cx).to_string(),
            focus: design::focus(cx).to_string(),
            text_1: design::t1(cx).to_string(),
            text_2: design::t2(cx).to_string(),
            text_3: design::t3(cx).to_string(),
            text_4: design::t4(cx).to_string(),
            line: design::line(cx).to_string(),
            line_2: design::line_2(cx).to_string(),
            accent: design::accent(cx).to_string(),
            accent_2: design::accent_2(cx).to_string(),
            on_accent: design::on_accent(cx).to_string(),
            accent_soft: design::accent_soft(cx).to_string(),
            accent_line: design::accent_line(cx).to_string(),
            info: design::sky(cx).to_string(),
            overlay: design::sink(cx).opacity(0.72).to_string(),
            shadow: design::sink(cx).opacity(0.60).to_string(),
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

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PenpotMessage {
    OpenAssistant,
    McpStatus {
        connected: bool,
        #[serde(rename = "fileId")]
        file_id: Option<Uuid>,
        #[serde(rename = "surfaceId")]
        surface_id: Uuid,
    },
    ExportFinished {
        success: bool,
        #[serde(rename = "fileName")]
        file_name: String,
    },
}

fn penpot_message_matches_surface(
    message: &PenpotMessage,
    active_surface_id: Option<Uuid>,
) -> bool {
    match message {
        PenpotMessage::McpStatus { surface_id, .. } => Some(*surface_id) == active_surface_id,
        PenpotMessage::OpenAssistant | PenpotMessage::ExportFinished { .. } => {
            active_surface_id.is_some()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PenpotSidebarTab {
    Layers,
    Assets,
    Tokens,
}

impl PenpotSidebarTab {
    fn as_str(self) -> &'static str {
        match self {
            Self::Layers => "layers",
            Self::Assets => "assets",
            Self::Tokens => "tokens",
        }
    }
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
    use std::sync::{Arc, Mutex};

    use base64::Engine as _;
    use block2::RcBlock;
    use cef::rc::Rc as _;
    use cef::{
        browser_host_create_browser, BeforeDownloadCallback, Browser, BrowserSettings, CefString,
        Client, DisplayHandler, DownloadHandler, DownloadItem, DownloadItemCallback, Frame,
        ImplBeforeDownloadCallback, ImplBrowser, ImplBrowserHost, ImplClient, ImplDisplayHandler,
        ImplDownloadHandler, ImplDownloadItem, ImplFrame, ImplLifeSpanHandler, ImplLoadHandler,
        LifeSpanHandler, LoadHandler, LogSeverity, RuntimeStyle, State, WindowInfo, WrapClient,
        WrapDisplayHandler, WrapDownloadHandler, WrapLifeSpanHandler, WrapLoadHandler,
    };
    use gpui::{Bounds, Pixels, Window};
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObject, ProtocolObject};
    use objc2::{
        define_class, msg_send, ClassType, DeclaredClass, MainThreadMarker, MainThreadOnly,
    };
    use objc2_app_kit::{
        NSAutoresizingMaskOptions, NSBitmapImageFileType, NSBitmapImageRep,
        NSBitmapImageRepPropertyKey, NSEvent, NSEventModifierFlags, NSEventType, NSImage,
        NSPasteboard, NSPasteboardWriting, NSView,
    };
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};
    use objc2_foundation::{
        NSArray, NSDictionary, NSError, NSObjectProtocol, NSPoint, NSProcessInfo, NSString, NSURL,
    };
    use objc2_web_kit::{
        WKContentWorld, WKScriptMessage, WKScriptMessageHandler, WKSnapshotConfiguration,
        WKUserContentController, WKUserScript, WKUserScriptInjectionTime,
    };
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use wry::dpi::{LogicalPosition, LogicalSize};
    use wry::http::{header::CONTENT_TYPE, Request, Response};
    use wry::{PageLoadEvent, Rect, WebView, WebViewBuilder, WebViewExtMacOS};

    use super::{
        penpot_message_matches_surface, AsyncApp, DocEditorMessage, PenpotMessage,
        PenpotSidebarTab, ProjectId, ProjectPreviewConsoleEntry, ProjectPreviewInspectorMessage,
        ProjectPreviewMessage, Uuid, VisualizationTheme, WebPreviewIntent, MAX_VISUALIZATION_BYTES,
    };

    const MAX_DOC_UPLOAD_BYTES: usize = 25 * 1024 * 1024;
    const MAX_DOC_EDITOR_MESSAGE_BYTES: usize = 40 * 1024 * 1024;
    const MAX_PENPOT_MESSAGE_BYTES: usize = 4 * 1024;
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
    const CHROMIUM_IPC_PREFIX: &str = "__CHORO_DESIGN_IPC__";

    struct ProjectPreviewMessageHandlerIvars {
        project_id: ProjectId,
        messages: Rc<RefCell<VecDeque<ProjectPreviewMessage>>>,
        app: AsyncApp,
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
        app: &AsyncApp,
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
        app: AsyncApp,
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
            controller: Retained<WKUserContentController>,
            project_id: ProjectId,
            messages: Rc<RefCell<VecDeque<ProjectPreviewMessage>>>,
            app: AsyncApp,
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
            let world = unsafe {
                WKContentWorld::worldWithName(
                    &NSString::from_str(PROJECT_PREVIEW_CONTENT_WORLD),
                    main_thread,
                )
            };
            let user_script = unsafe {
                WKUserScript::initWithSource_injectionTime_forMainFrameOnly_inContentWorld(
                    main_thread.alloc::<WKUserScript>(),
                    &NSString::from_str(PROJECT_PREVIEW_INSPECTOR_JS),
                    WKUserScriptInjectionTime::AtDocumentStart,
                    true,
                    &world,
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
                    &world,
                    &NSString::from_str(PROJECT_PREVIEW_MESSAGE_HANDLER),
                );
                controller.addUserScript(&user_script);
                controller.addUserScript(&agent_script);
            }
        }
    }

    impl ProjectPreviewConsoleMessageHandler {
        fn install(
            controller: Retained<WKUserContentController>,
            project_id: ProjectId,
            messages: Rc<RefCell<VecDeque<ProjectPreviewMessage>>>,
            app: AsyncApp,
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
            let page_world = unsafe { WKContentWorld::pageWorld(main_thread) };
            let user_script = unsafe {
                WKUserScript::initWithSource_injectionTime_forMainFrameOnly_inContentWorld(
                    main_thread.alloc::<WKUserScript>(),
                    &NSString::from_str(PROJECT_PREVIEW_CONSOLE_JS),
                    WKUserScriptInjectionTime::AtDocumentStart,
                    true,
                    &page_world,
                )
            };
            unsafe {
                // This page-world bridge accepts console entries only. Keeping
                // it separate prevents previewed code from forging inspector or
                // agent-control messages that live in Choro's isolated world.
                controller.addScriptMessageHandler_contentWorld_name(
                    protocol_handler,
                    &page_world,
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
        parked_penpot: Option<Active>,
        pending: Option<WebPreviewIntent>,
        placed_since_reconcile: bool,
        suspended: bool,
        overlay_suspended: bool,
        penpot_keepalive: bool,
        penpot_assistant_open: bool,
        penpot_compare_open: bool,
        messages: Rc<RefCell<VecDeque<DocEditorMessage>>>,
        penpot_messages: Arc<Mutex<VecDeque<PenpotMessage>>>,
        preview_messages: Rc<RefCell<VecDeque<ProjectPreviewMessage>>>,
        project_preview_live_urls: Rc<RefCell<HashMap<ProjectId, String>>>,
        app: AsyncApp,
    }

    struct Active {
        intent: WebPreviewIntent,
        webview: WebSurface,
        surface_id: Option<Uuid>,
        bounds: Bounds<Pixels>,
        doc_editor_ready: bool,
        penpot_assistant_open: bool,
        penpot_compare_open: bool,
    }

    enum WebSurface {
        WebKit(WebView),
        Chromium(ChromiumSurface),
    }

    impl WebSurface {
        fn set_visible(&self, visible: bool) -> Result<(), String> {
            match self {
                Self::WebKit(webview) => webview
                    .set_visible(visible)
                    .map_err(|error| error.to_string()),
                Self::Chromium(surface) => surface.set_visible(visible),
            }
        }

        fn set_bounds(&self, bounds: Bounds<Pixels>) -> Result<(), String> {
            match self {
                Self::WebKit(webview) => webview
                    .set_bounds(to_rect(bounds))
                    .map_err(|error| error.to_string()),
                Self::Chromium(surface) => surface.set_bounds(bounds),
            }
        }

        fn load_url(&self, url: &str) -> Result<(), String> {
            match self {
                Self::WebKit(webview) => webview.load_url(url).map_err(|error| error.to_string()),
                Self::Chromium(surface) => surface.load_url(url),
            }
        }

        fn evaluate_script(&self, script: &str) -> Result<(), String> {
            match self {
                Self::WebKit(webview) => webview
                    .evaluate_script(script)
                    .map_err(|error| error.to_string()),
                Self::Chromium(surface) => surface.evaluate_script(script),
            }
        }

        fn reload_from_origin(&self) -> Result<(), String> {
            match self {
                Self::WebKit(webview) => {
                    // Reload the document WebKit is currently showing, not the
                    // configured Preview intent URL. `reloadFromOrigin` also
                    // revalidates resources so this acts like a real browser
                    // refresh while debugging local changes.
                    unsafe {
                        webview.webview().reloadFromOrigin();
                    }
                    Ok(())
                }
                Self::Chromium(surface) => surface.reload_from_origin(),
            }
        }

        fn current_url(&self) -> Option<String> {
            match self {
                Self::WebKit(webview) => unsafe {
                    webview
                        .webview()
                        .URL()
                        .and_then(|url| url.absoluteString())
                        .map(|url| url.to_string())
                },
                Self::Chromium(surface) => surface.current_url(),
            }
        }

        fn webkit(&self) -> Option<&WebView> {
            match self {
                Self::WebKit(webview) => Some(webview),
                Self::Chromium(_) => None,
            }
        }
    }

    struct ChromiumSurfaceState {
        browser: Option<Browser>,
        pending_url: Option<String>,
        pending_scripts: VecDeque<String>,
        initialization_script: String,
        closing: bool,
        downloads: HashMap<u32, PathBuf>,
        messages: Arc<Mutex<VecDeque<PenpotMessage>>>,
        app: AsyncApp,
    }

    struct ChromiumSurface {
        state: Arc<Mutex<ChromiumSurfaceState>>,
        container: Retained<NSView>,
    }

    impl ChromiumSurface {
        fn new(
            url: &str,
            initialization_script: String,
            bounds: Bounds<Pixels>,
            window: &Window,
            messages: Arc<Mutex<VecDeque<PenpotMessage>>>,
            app: AsyncApp,
        ) -> Result<Self, String> {
            if !crate::chromium::is_ready() {
                return Err("Chromium has not finished initializing".to_string());
            }
            let main_thread = MainThreadMarker::new()
                .ok_or_else(|| "Design must create Chromium on the main thread".to_string())?;
            let handle = HasWindowHandle::window_handle(window)
                .map_err(|error| format!("could not read Choro's native window: {error}"))?;
            let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
                return Err("Choro did not provide an AppKit window".to_string());
            };
            let parent = unsafe { &*(appkit.ns_view.as_ptr() as *const NSView) };
            let frame = appkit_frame(parent, bounds);
            let container = NSView::initWithFrame(main_thread.alloc::<NSView>(), frame);
            container.setAutoresizesSubviews(true);
            parent.addSubview(&container);

            let state = Arc::new(Mutex::new(ChromiumSurfaceState {
                browser: None,
                pending_url: None,
                pending_scripts: VecDeque::new(),
                initialization_script,
                closing: false,
                downloads: HashMap::new(),
                messages,
                app,
            }));
            let mut client = ChoroDesignClient::new(state.clone());
            let cef_bounds = cef::Rect {
                x: 0,
                y: 0,
                width: frame.size.width.max(1.0).round() as i32,
                height: frame.size.height.max(1.0).round() as i32,
            };
            let window_info = WindowInfo {
                runtime_style: RuntimeStyle::ALLOY,
                ..Default::default()
            }
            .set_as_child(
                Retained::as_ptr(&container) as *mut std::ffi::c_void,
                &cef_bounds,
            );
            let browser_settings = BrowserSettings {
                webgl: State::ENABLED,
                javascript_access_clipboard: State::ENABLED,
                ..Default::default()
            };
            crate::chromium::browser_creation_started();
            if browser_host_create_browser(
                Some(&window_info),
                Some(&mut client),
                Some(&CefString::from(url)),
                Some(&browser_settings),
                None,
                None,
            ) != 1
            {
                crate::chromium::browser_creation_failed();
                container.removeFromSuperview();
                return Err("CEF rejected the Design browser creation request".to_string());
            }

            Ok(Self { state, container })
        }

        fn browser(&self) -> Option<Browser> {
            self.state.lock().ok()?.browser.clone()
        }

        fn set_visible(&self, visible: bool) -> Result<(), String> {
            self.container.setHidden(!visible);
            Ok(())
        }

        fn set_bounds(&self, bounds: Bounds<Pixels>) -> Result<(), String> {
            let parent = unsafe { self.container.superview() }
                .ok_or_else(|| "Chromium Design surface was detached".to_string())?;
            self.container.setFrame(appkit_frame(&parent, bounds));
            if let Some(browser) = self.browser() {
                if let Some(host) = browser.host() {
                    let view = host.window_handle() as *mut NSView;
                    if !view.is_null() {
                        unsafe {
                            (&*view).setFrame(self.container.bounds());
                        }
                    }
                }
            }
            Ok(())
        }

        fn load_url(&self, url: &str) -> Result<(), String> {
            if let Some(frame) = self.browser().and_then(|browser| browser.main_frame()) {
                frame.load_url(Some(&CefString::from(url)));
                return Ok(());
            }
            let mut state = self
                .state
                .lock()
                .map_err(|_| "Chromium Design state was unavailable".to_string())?;
            state.pending_url = Some(url.to_string());
            Ok(())
        }

        fn evaluate_script(&self, script: &str) -> Result<(), String> {
            if let Some(frame) = self.browser().and_then(|browser| browser.main_frame()) {
                execute_chromium_script(&frame, script);
                return Ok(());
            }
            let mut state = self
                .state
                .lock()
                .map_err(|_| "Chromium Design state was unavailable".to_string())?;
            state.pending_scripts.push_back(script.to_string());
            Ok(())
        }

        fn reload_from_origin(&self) -> Result<(), String> {
            let browser = self
                .browser()
                .ok_or_else(|| "Chromium Design is still starting".to_string())?;
            browser.reload_ignore_cache();
            Ok(())
        }

        fn current_url(&self) -> Option<String> {
            self.browser()
                .and_then(|browser| browser.main_frame())
                .map(|frame| {
                    let url = frame.url();
                    CefString::from(&url).to_string()
                })
                .filter(|url| !url.is_empty())
        }
    }

    impl Drop for ChromiumSurface {
        fn drop(&mut self) {
            self.container.setHidden(true);
            self.container.removeFromSuperview();
            let browser = self.state.lock().ok().and_then(|mut state| {
                state.closing = true;
                state.browser.clone()
            });
            if let Some(host) = browser.and_then(|browser| browser.host()) {
                host.close_browser(1);
            }
        }
    }

    fn appkit_frame(parent: &NSView, bounds: Bounds<Pixels>) -> CGRect {
        let x = f32::from(bounds.origin.x) as f64;
        let y = f32::from(bounds.origin.y) as f64;
        let width = (f32::from(bounds.size.width) as f64).max(1.0);
        let height = (f32::from(bounds.size.height) as f64).max(1.0);
        let origin_y = if parent.isFlipped() {
            y
        } else {
            parent.frame().size.height - y - height
        };
        CGRect::new(CGPoint::new(x, origin_y), CGSize::new(width, height))
    }

    fn execute_chromium_script(frame: &Frame, script: &str) {
        frame.execute_java_script(
            Some(&CefString::from(script)),
            Some(&CefString::from("choro://design")),
            0,
        );
    }

    fn report_chromium_export(
        state: &Arc<Mutex<ChromiumSurfaceState>>,
        success: bool,
        file_name: String,
    ) {
        let (messages, app) = match state.lock() {
            Ok(state) => (state.messages.clone(), state.app.clone()),
            Err(_) => return,
        };
        if let Ok(mut messages) = messages.lock() {
            messages.push_back(PenpotMessage::ExportFinished { success, file_name });
        }
        let _ = app.refresh();
    }

    cef::wrap_client! {
        struct ChoroDesignClient {
            state: Arc<Mutex<ChromiumSurfaceState>>,
        }

        impl Client {
            fn display_handler(&self) -> Option<DisplayHandler> {
                Some(ChoroDesignDisplayHandler::new(self.state.clone()))
            }

            fn download_handler(&self) -> Option<DownloadHandler> {
                Some(ChoroDesignDownloadHandler::new(self.state.clone()))
            }

            fn life_span_handler(&self) -> Option<LifeSpanHandler> {
                Some(ChoroDesignLifeSpanHandler::new(self.state.clone()))
            }

            fn load_handler(&self) -> Option<LoadHandler> {
                Some(ChoroDesignLoadHandler::new(self.state.clone()))
            }
        }
    }

    cef::wrap_display_handler! {
        struct ChoroDesignDisplayHandler {
            state: Arc<Mutex<ChromiumSurfaceState>>,
        }

        impl DisplayHandler {
            fn on_console_message(
                &self,
                _browser: Option<&mut Browser>,
                _level: LogSeverity,
                message: Option<&CefString>,
                _source: Option<&CefString>,
                _line: i32,
            ) -> i32 {
                let Some(body) = message
                    .map(CefString::to_string)
                    .and_then(|message| message.strip_prefix(CHROMIUM_IPC_PREFIX).map(str::to_owned))
                else {
                    return 0;
                };
                if body.len() > MAX_PENPOT_MESSAGE_BYTES {
                    eprintln!("Design Chromium message exceeded the size limit");
                    return 1;
                }
                let message = match serde_json::from_str::<PenpotMessage>(&body) {
                    Ok(message) => message,
                    Err(error) => {
                        eprintln!("invalid Design Chromium message: {error}");
                        return 1;
                    }
                };
                let (messages, app) = match self.state.lock() {
                    Ok(state) => (state.messages.clone(), state.app.clone()),
                    Err(_) => return 1,
                };
                if let Ok(mut messages) = messages.lock() {
                    messages.push_back(message);
                }
                let _ = app.refresh();
                1
            }
        }
    }

    cef::wrap_life_span_handler! {
        struct ChoroDesignLifeSpanHandler {
            state: Arc<Mutex<ChromiumSurfaceState>>,
        }

        impl LifeSpanHandler {
            fn on_after_created(&self, browser: Option<&mut Browser>) {
                let Some(browser) = browser else {
                    return;
                };
                crate::chromium::register_browser(browser);
                if let Some(host) = browser.host() {
                    let view = host.window_handle() as *mut NSView;
                    if !view.is_null() {
                        unsafe {
                            (&*view).setAutoresizingMask(
                                NSAutoresizingMaskOptions::ViewWidthSizable
                                    | NSAutoresizingMaskOptions::ViewHeightSizable,
                            );
                        }
                    }
                }

                let (closing, pending_url, pending_scripts) = match self.state.lock() {
                    Ok(mut state) => {
                        state.browser = Some(browser.clone());
                        (
                            state.closing,
                            state.pending_url.take(),
                            state.pending_scripts.drain(..).collect::<Vec<_>>(),
                        )
                    }
                    Err(_) => return,
                };
                if closing {
                    if let Some(host) = browser.host() {
                        host.close_browser(1);
                    }
                    return;
                }
                let Some(frame) = browser.main_frame() else {
                    return;
                };
                if let Some(url) = pending_url {
                    frame.load_url(Some(&CefString::from(url.as_str())));
                }
                for script in pending_scripts {
                    execute_chromium_script(&frame, &script);
                }
            }

            fn on_before_close(&self, browser: Option<&mut Browser>) {
                let Some(browser) = browser else {
                    return;
                };
                crate::chromium::unregister_browser(browser);
                if let Ok(mut state) = self.state.lock() {
                    state.browser = None;
                }
            }
        }
    }

    cef::wrap_load_handler! {
        struct ChoroDesignLoadHandler {
            state: Arc<Mutex<ChromiumSurfaceState>>,
        }

        impl LoadHandler {
            fn on_load_end(
                &self,
                _browser: Option<&mut Browser>,
                frame: Option<&mut Frame>,
                _http_status_code: i32,
            ) {
                let Some(frame) = frame.filter(|frame| frame.is_main() != 0) else {
                    return;
                };
                let script = match self.state.lock() {
                    Ok(state) => state.initialization_script.clone(),
                    Err(_) => return,
                };
                execute_chromium_script(frame, &script);
            }
        }
    }

    cef::wrap_download_handler! {
        struct ChoroDesignDownloadHandler {
            state: Arc<Mutex<ChromiumSurfaceState>>,
        }

        impl DownloadHandler {
            fn can_download(
                &self,
                _browser: Option<&mut Browser>,
                _url: Option<&CefString>,
                _request_method: Option<&CefString>,
            ) -> i32 {
                1
            }

            fn on_before_download(
                &self,
                _browser: Option<&mut Browser>,
                download_item: Option<&mut DownloadItem>,
                suggested_name: Option<&CefString>,
                callback: Option<&mut BeforeDownloadCallback>,
            ) -> i32 {
                let (Some(download_item), Some(callback)) = (download_item, callback) else {
                    return 0;
                };
                let suggested_name = suggested_name
                    .map(CefString::to_string)
                    .filter(|name| !name.trim().is_empty())
                    .unwrap_or_else(|| {
                        let name = download_item.suggested_file_name();
                        CefString::from(&name).to_string()
                    });
                let suggested_path = Path::new(&suggested_name);
                let target = match design_download_destination(suggested_path) {
                    Ok(target) => target,
                    Err(error) => {
                        eprintln!("could not prepare Chromium Design export: {error}");
                        report_chromium_export(
                            &self.state,
                            false,
                            design_export_file_name(suggested_path),
                        );
                        return 0;
                    }
                };
                if let Ok(mut state) = self.state.lock() {
                    state.downloads.insert(download_item.id(), target.clone());
                }
                callback.cont(
                    Some(&CefString::from(target.to_string_lossy().as_ref())),
                    0,
                );
                1
            }

            fn on_download_updated(
                &self,
                _browser: Option<&mut Browser>,
                download_item: Option<&mut DownloadItem>,
                _callback: Option<&mut DownloadItemCallback>,
            ) {
                let Some(download_item) = download_item else {
                    return;
                };
                let success = download_item.is_complete() != 0;
                let finished = success
                    || download_item.is_canceled() != 0
                    || download_item.is_interrupted() != 0;
                if !finished {
                    return;
                }
                let target = self
                    .state
                    .lock()
                    .ok()
                    .and_then(|mut state| state.downloads.remove(&download_item.id()));
                let Some(target) = target else {
                    return;
                };
                let file_name = design_export_file_name(&target);
                if success {
                    eprintln!("Design export saved to {}", target.display());
                } else {
                    eprintln!("Design export failed: {file_name}");
                }
                report_chromium_export(&self.state, success, file_name);
            }
        }
    }

    impl WebPreviewHost {
        pub fn new(app: AsyncApp) -> Self {
            Self {
                active: None,
                parked_penpot: None,
                pending: None,
                placed_since_reconcile: false,
                suspended: false,
                overlay_suspended: false,
                penpot_keepalive: false,
                penpot_assistant_open: false,
                penpot_compare_open: false,
                messages: Rc::new(RefCell::new(VecDeque::new())),
                penpot_messages: Arc::new(Mutex::new(VecDeque::new())),
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
                let _ = active.webview.set_visible(surface_visible(
                    self.suspended || self.overlay_suspended,
                    active.doc_editor_ready,
                ));
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
                let _ = active.webview.set_visible(surface_visible(
                    self.suspended || self.overlay_suspended,
                    active.doc_editor_ready,
                ));
            }
        }

        /// Select the single web surface for this frame. If an active chat row
        /// was not placed during the previous frame, tear it down and leave the
        /// intent pending until that row becomes visible again.
        pub fn set_intent(&mut self, intent: Option<WebPreviewIntent>) -> bool {
            let had_active = self.active.is_some();
            self.cache_active_project_preview_live_url();

            let incoming_penpot = intent
                .as_ref()
                .is_some_and(|intent| matches!(intent, WebPreviewIntent::PenpotUrl { .. }));
            if !self.penpot_keepalive && !incoming_penpot {
                self.parked_penpot = None;
            }

            let should_park_active_penpot = self.penpot_keepalive
                && self.active.as_ref().is_some_and(|active| {
                    matches!(active.intent, WebPreviewIntent::PenpotUrl { .. })
                        && intent.as_ref().is_none_or(|intent| {
                            !matches!(intent, WebPreviewIntent::PenpotUrl { .. })
                        })
                });
            if should_park_active_penpot {
                if let Some(active) = self.active.take() {
                    let _ = active.webview.set_visible(false);
                    self.parked_penpot = Some(active);
                }
            }

            let Some(intent) = intent else {
                self.active = None;
                self.pending = None;
                self.placed_since_reconcile = false;
                return had_active;
            };

            if matches!(intent, WebPreviewIntent::PenpotUrl { .. })
                && !self
                    .active
                    .as_ref()
                    .is_some_and(|active| same_surface(&active.intent, &intent))
            {
                if let Some(mut parked) = self.parked_penpot.take() {
                    if same_surface(&parked.intent, &intent) {
                        self.active = None;
                        sync_active_penpot_theme(&parked, &intent);
                        parked.intent = intent;
                        let _ = parked.webview.set_visible(false);
                        self.active = Some(parked);
                        self.pending = None;
                        self.placed_since_reconcile = false;
                        return had_active;
                    }
                }
            }

            if self
                .active
                .as_ref()
                .is_some_and(|active| same_surface(&active.intent, &intent))
            {
                if self.active.as_ref().map(|active| &active.intent) != Some(&intent) {
                    if let Some(active) = self.active.as_mut() {
                        sync_active_doc_editor(active, &intent);
                        sync_active_penpot_theme(active, &intent);
                        active.intent = intent.clone();
                    }
                }
                if self.placed_since_reconcile {
                    self.placed_since_reconcile = false;
                    return false;
                }
                self.active = None;
                self.pending = Some(intent);
                return true;
            }

            if self.pending.as_ref() == Some(&intent) {
                self.placed_since_reconcile = false;
                return false;
            }

            self.active = None;
            self.pending = Some(intent);
            self.placed_since_reconcile = false;
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
                        let visible = !(self.suspended || self.overlay_suspended);
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

        pub fn take_penpot_messages(&mut self) -> Vec<PenpotMessage> {
            let active_surface_id = self
                .active
                .as_ref()
                .filter(|active| matches!(active.intent, WebPreviewIntent::PenpotUrl { .. }))
                .and_then(|active| active.surface_id)
                .or_else(|| {
                    self.parked_penpot
                        .as_ref()
                        .and_then(|active| active.surface_id)
                });
            self.penpot_messages
                .lock()
                .map(|mut messages| {
                    messages
                        .drain(..)
                        .filter(|message| {
                            penpot_message_matches_surface(message, active_surface_id)
                        })
                        .collect()
                })
                .unwrap_or_default()
        }

        pub fn navigate_url(&self, url: &str) -> Result<(), String> {
            let Some(active) = self.active.as_ref() else {
                return Err("The Design canvas is still loading.".to_string());
            };
            if !matches!(active.intent, WebPreviewIntent::PenpotUrl { .. }) {
                return Err("The active web view cannot navigate to this design.".to_string());
            }
            active
                .webview
                .load_url(url)
                .map_err(|error| format!("Could not open the design: {error}"))
        }

        pub fn set_penpot_assistant_open(&mut self, open: bool) {
            self.penpot_assistant_open = open;
        }

        /// Put the embedded Penpot workspace into the reduced-chrome Compare
        /// layout. The left workspace sidebar and the right inspector are
        /// hidden while the live project preview shares the center area.
        pub fn set_penpot_compare_open(&mut self, open: bool) {
            self.penpot_compare_open = open;
        }

        /// Keep the live Penpot surface connected while a design-linked agent
        /// is open. Penpot's MCP server delegates canvas inspection to the
        /// browser plugin instance, so destroying the hidden WKWebView would
        /// leave the agent with a valid token but no connected plugin.
        pub fn set_penpot_keepalive(&mut self, keepalive: bool) {
            self.penpot_keepalive = keepalive;
        }

        pub fn select_penpot_sidebar_tab(&self, tab: PenpotSidebarTab) {
            let Some(active) = self.active.as_ref() else {
                return;
            };
            if matches!(active.intent, WebPreviewIntent::PenpotUrl { .. }) {
                let _ = active
                    .webview
                    .evaluate_script(&penpot_sidebar_tab_script(tab));
            }
        }

        pub fn collapse_penpot_left_sidebar(&self) {
            let Some(active) = self.active.as_ref() else {
                return;
            };
            if matches!(active.intent, WebPreviewIntent::PenpotUrl { .. }) {
                let _ = active
                    .webview
                    .evaluate_script(penpot_left_sidebar_collapse_script());
            }
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

        /// Build or reposition the selected web surface inside a GPUI-reserved
        /// rectangle. Only a visible chat row calls this method.
        pub fn place(&mut self, bounds: Bounds<Pixels>, window: &Window) {
            self.placed_since_reconcile = true;
            if self.suspended || self.overlay_suspended {
                if let Some(active) = self.active.as_ref() {
                    let _ = active.webview.set_visible(false);
                }
                return;
            }
            if let Some(intent) = self.pending.take() {
                let surface_id =
                    matches!(intent, WebPreviewIntent::PenpotUrl { .. }).then(Uuid::new_v4);
                match build(
                    &intent,
                    bounds,
                    window,
                    self.messages.clone(),
                    self.penpot_messages.clone(),
                    self.preview_messages.clone(),
                    self.project_preview_live_urls.clone(),
                    self.app.clone(),
                    self.penpot_assistant_open,
                    self.penpot_compare_open,
                    surface_id,
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
                            surface_id,
                            bounds,
                            doc_editor_ready,
                            penpot_assistant_open: self.penpot_assistant_open,
                            penpot_compare_open: self.penpot_compare_open,
                        });
                    }
                    Err(error) => eprintln!("web preview build failed: {error}"),
                }
            } else if let Some(active) = &mut self.active {
                let _ = active
                    .webview
                    .set_visible(surface_visible(false, active.doc_editor_ready));
                let penpot_assistant_changed =
                    matches!(&active.intent, WebPreviewIntent::PenpotUrl { .. })
                        && active.penpot_assistant_open != self.penpot_assistant_open;
                let penpot_compare_changed =
                    matches!(&active.intent, WebPreviewIntent::PenpotUrl { .. })
                        && active.penpot_compare_open != self.penpot_compare_open;
                if active.bounds != bounds {
                    active.bounds = bounds;
                    let _ = active.webview.set_bounds(bounds);
                    if matches!(&active.intent, WebPreviewIntent::PenpotUrl { .. }) {
                        // WKWebView updates its native frame here, but WebKit does
                        // not reliably emit a DOM resize event for child-view
                        // frame changes. Penpot caches its workspace measurements,
                        // which can leave its canvas and sidebars laid out against
                        // the previous frame after Choro swaps the Agent sidebar.
                        let _ = active.webview.evaluate_script(
                            "window.dispatchEvent(new Event('resize'));\
                             window.requestAnimationFrame(() => \
                               window.dispatchEvent(new Event('resize')));\
                             window.setTimeout(() => \
                               window.dispatchEvent(new Event('resize')), 120);",
                        );
                    }
                }
                if penpot_assistant_changed || penpot_compare_changed {
                    active.penpot_assistant_open = self.penpot_assistant_open;
                    active.penpot_compare_open = self.penpot_compare_open;
                    let _ = active.webview.evaluate_script(&penpot_chrome_sync_script(
                        self.penpot_assistant_open,
                        self.penpot_compare_open,
                    ));
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
            (
                WebPreviewIntent::PenpotUrl { url: left, .. },
                WebPreviewIntent::PenpotUrl { url: right, .. },
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
            _ => false,
        }
    }

    fn design_export_file_name(path: &Path) -> String {
        path.file_name()
            .and_then(|name| name.to_str())
            .map(str::trim)
            .filter(|name| !name.is_empty() && *name != "." && *name != "..")
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| "design-export".to_string())
    }

    fn design_download_destination(suggested_path: &Path) -> io::Result<PathBuf> {
        let directory = dirs::download_dir()
            .or_else(dirs::desktop_dir)
            .unwrap_or_else(std::env::temp_dir);
        fs::create_dir_all(&directory)?;

        let file_name = design_export_file_name(suggested_path);
        Ok(unique_design_download_path(&directory, &file_name))
    }

    fn unique_design_download_path(directory: &Path, file_name: &str) -> PathBuf {
        let candidate = directory.join(file_name);
        if !candidate.exists() {
            return candidate;
        }

        let file = Path::new(file_name);
        let stem = file
            .file_stem()
            .and_then(|stem| stem.to_str())
            .filter(|stem| !stem.is_empty())
            .unwrap_or("design-export");
        let extension = file.extension().and_then(|extension| extension.to_str());

        for index in 1_u64.. {
            let next_name = match extension {
                Some(extension) if !extension.is_empty() => {
                    format!("{stem} ({index}).{extension}")
                }
                _ => format!("{stem} ({index})"),
            };
            let candidate = directory.join(next_name);
            if !candidate.exists() {
                return candidate;
            }
        }

        unreachable!("the design export suffix space is unbounded")
    }

    fn penpot_chrome_sync_script(assistant_open: bool, compare_open: bool) -> String {
        format!(
            r##"(() => {{
                const assistantOpen = {assistant_open};
                const compareOpen = {compare_open};
                window.__choroAssistantOpen = assistantOpen;
                window.__choroCompareOpen = compareOpen;
                try {{
                    window.sessionStorage.setItem("choro.assistant.open", String(assistantOpen));
                }} catch (_) {{}}
                const styleId = "choro-embedded-penpot-compare";
                let style = document.getElementById(styleId);
                if (!style) {{
                    style = document.createElement("style");
                    style.id = styleId;
                    (document.head || document.documentElement).appendChild(style);
                }}
                const hiddenChrome = [];
                if (assistantOpen || compareOpen) {{
                    hiddenChrome.push(
                        "#left-sidebar-aside, [data-testid='left-sidebar'] {{ display: none !important; }}"
                    );
                }}
                if (compareOpen) {{
                    hiddenChrome.push(
                        "#right-sidebar-aside, [data-testid='right-sidebar'] {{ display: none !important; }}"
                    );
                }}
                style.textContent = hiddenChrome.join("\n");
                let attempts = 0;
                const sync = () => {{
                    const api = window.choroPenpot;
                    if (api && typeof api.setCompareMode === "function") {{
                        api.setCompareMode(Boolean(window.__choroCompareOpen));
                    }}
                    if (api && typeof api.setAssistantOpen === "function") {{
                        api.setAssistantOpen(Boolean(
                            window.__choroAssistantOpen || window.__choroCompareOpen
                        ));
                    }}
                    if (api && (
                        typeof api.setCompareMode === "function" ||
                        typeof api.setAssistantOpen === "function"
                    )) {{
                        window.dispatchEvent(new Event("resize"));
                        window.requestAnimationFrame(() => window.dispatchEvent(new Event("resize")));
                        return;
                    }}
                    if (attempts++ < 240) window.setTimeout(sync, 250);
                }};
                sync();
            }})();"##
        )
    }

    fn penpot_theme_sync_script(theme: &super::PenpotTheme) -> String {
        let theme = serde_json::to_string(theme).expect("Design theme serializes to JSON");
        format!(
            r#"(() => {{
                const theme = {theme};
                window.__choroTheme = theme;
                let attempts = 0;
                const sync = () => {{
                    const api = window.choroPenpot;
                    if (api && typeof api.setTheme === "function") {{
                        api.setTheme(theme);
                        return;
                    }}
                    if (attempts++ < 240) window.setTimeout(sync, 250);
                }};
                sync();
            }})();"#
        )
    }

    /// Hosted below GPUI's AppKit view hierarchy, the embedded browser's
    /// text-input interpretation loses the native editing commands for
    /// non-printing keys and surfaces them as literal characters instead:
    /// macOS function-key code points (U+F700–U+F8FF, arrows/Home/End/Delete)
    /// or WebKit's legacy separators (U+001C–U+001F). Penpot's text editor
    /// applies those to its model as visible blank glyphs, while the caret
    /// does not move. Each orphaned character event fires exactly once per
    /// key press the native stack dropped, so the guard both blocks the
    /// insertion and performs the intended edit itself.
    fn penpot_embedded_key_guard_script() -> &'static str {
        r#"
                const installEmbeddedKeyGuard = () => {
                    if (window.__choroEmbeddedKeyGuard) return;
                    window.__choroEmbeddedKeyGuard = true;
                    const guardedCode = (event) => {
                        let code = event.charCode || 0;
                        if (!code && typeof event.key === "string" && event.key.length === 1) {
                            code = event.key.codePointAt(0);
                        }
                        if (code >= 0x001c && code <= 0x001f) {
                            code = [0xf702, 0xf703, 0xf700, 0xf701][code - 0x001c];
                        }
                        return code >= 0xf700 && code <= 0xf8ff ? code : null;
                    };
                    const badText = (text) => {
                        if (typeof text !== "string" || !text.length) return false;
                        for (const character of text) {
                            const code = character.codePointAt(0);
                            if ((code >= 0xf700 && code <= 0xf8ff) ||
                                (code >= 0x001c && code <= 0x001f)) {
                                return true;
                            }
                        }
                        return false;
                    };
                    const wordStep = (value, position, forward) => {
                        let index = position;
                        if (forward) {
                            while (index < value.length && /\s/.test(value[index])) index += 1;
                            while (index < value.length && !/\s/.test(value[index])) index += 1;
                        } else {
                            while (index > 0 && /\s/.test(value[index - 1])) index -= 1;
                            while (index > 0 && !/\s/.test(value[index - 1])) index -= 1;
                        }
                        return index;
                    };
                    const lineStep = (value, caret, forward) => {
                        const lineStart = value.lastIndexOf("\n", caret - 1) + 1;
                        const column = caret - lineStart;
                        if (!forward) {
                            if (lineStart === 0) return 0;
                            const previousStart = value.lastIndexOf("\n", lineStart - 2) + 1;
                            return Math.min(previousStart + column, lineStart - 1);
                        }
                        const lineEnd = value.indexOf("\n", caret);
                        if (lineEnd === -1) return value.length;
                        const nextStart = lineEnd + 1;
                        let nextEnd = value.indexOf("\n", nextStart);
                        if (nextEnd === -1) nextEnd = value.length;
                        return Math.min(nextStart + column, nextEnd);
                    };
                    const moveInTextControl = (control, event, code) => {
                        if (![0xf700, 0xf701, 0xf702, 0xf703, 0xf729, 0xf72b].includes(code)) {
                            return;
                        }
                        const value = control.value || "";
                        const start = control.selectionStart;
                        const end = control.selectionEnd;
                        if (start === null || end === null) return;
                        const backwardSelection = control.selectionDirection === "backward";
                        const caret = backwardSelection ? start : end;
                        const anchor = backwardSelection ? end : start;
                        const forward =
                            code === 0xf703 || code === 0xf701 || code === 0xf72b;
                        const isTextarea = control.tagName === "TEXTAREA";
                        let target;
                        if (isTextarea && event.metaKey &&
                            (code === 0xf702 || code === 0xf703)) {
                            const lineStart = value.lastIndexOf("\n", caret - 1) + 1;
                            let lineEnd = value.indexOf("\n", caret);
                            if (lineEnd === -1) lineEnd = value.length;
                            target = forward ? lineEnd : lineStart;
                        } else if (code === 0xf729 || code === 0xf72b || event.metaKey) {
                            target = forward ? value.length : 0;
                        } else if (code === 0xf700 || code === 0xf701) {
                            target = isTextarea
                                ? lineStep(value, caret, forward)
                                : forward ? value.length : 0;
                        } else if (event.altKey) {
                            target = wordStep(value, caret, forward);
                        } else if (!event.shiftKey && start !== end) {
                            target = forward ? end : start;
                        } else {
                            target = forward
                                ? Math.min(value.length, caret + 1)
                                : Math.max(0, caret - 1);
                        }
                        if (event.shiftKey) {
                            control.setSelectionRange(
                                Math.min(anchor, target),
                                Math.max(anchor, target),
                                target < anchor ? "backward" : "forward"
                            );
                        } else {
                            control.setSelectionRange(target, target);
                        }
                    };
                    const moveInEditable = (event, code) => {
                        const selection = window.getSelection();
                        if (!selection || typeof selection.modify !== "function") return;
                        const alter = event.shiftKey ? "extend" : "move";
                        let direction;
                        let granularity;
                        if (code === 0xf702 || code === 0xf703) {
                            direction = code === 0xf702 ? "left" : "right";
                            granularity = event.metaKey
                                ? "lineboundary"
                                : event.altKey ? "word" : "character";
                        } else if (code === 0xf700 || code === 0xf701) {
                            direction = code === 0xf700 ? "backward" : "forward";
                            granularity = event.metaKey
                                ? "documentboundary"
                                : event.altKey ? "paragraphboundary" : "line";
                        } else if (code === 0xf729 || code === 0xf72b) {
                            direction = code === 0xf729 ? "left" : "right";
                            granularity = "lineboundary";
                        } else {
                            return;
                        }
                        selection.modify(alter, direction, granularity);
                    };
                    const editingFallback = (event, code) => {
                        try {
                            const active = document.activeElement;
                            if (!active) return;
                            const isTextControl =
                                active.tagName === "INPUT" ||
                                active.tagName === "TEXTAREA";
                            if (!isTextControl && !active.isContentEditable) return;
                            if (code === 0xf728) {
                                document.execCommand("forwardDelete");
                                return;
                            }
                            if (isTextControl) {
                                moveInTextControl(active, event, code);
                            } else {
                                moveInEditable(event, code);
                            }
                        } catch (_) {
                            // Selection APIs reject some control types; the
                            // guard must never break typing.
                        }
                    };
                    window.addEventListener("keypress", (event) => {
                        const code = guardedCode(event);
                        if (code === null) return;
                        event.preventDefault();
                        event.stopImmediatePropagation();
                        editingFallback(event, code);
                    }, true);
                    const blockTextEvent = (event) => {
                        if (badText(event.data)) {
                            event.preventDefault();
                            event.stopImmediatePropagation();
                        }
                    };
                    window.addEventListener("beforeinput", blockTextEvent, true);
                    window.addEventListener("textInput", blockTextEvent, true);
                };
                installEmbeddedKeyGuard();
        "#
    }

    fn penpot_initialization_script(
        assistant_open: bool,
        compare_open: bool,
        surface_id: Uuid,
        theme: &super::PenpotTheme,
        webkit_workarounds: bool,
    ) -> String {
        let surface_id =
            serde_json::to_string(&surface_id).unwrap_or_else(|_| "\"invalid\"".to_string());
        let theme = serde_json::to_string(theme).expect("Design theme serializes to JSON");
        let embedded_workarounds = if webkit_workarounds {
            format!(
                r##"{key_guard}
                const installEmbeddedTextEditorFix = () => {{
                    if (document.getElementById("choro-embedded-penpot-fixes")) return;
                    const style = document.createElement("style");
                    style.id = "choro-embedded-penpot-fixes";
                    style.textContent = `
                        /*
                         * Penpot positions the Safari 18/26 contenteditable
                         * wrapper as fixed to compensate for foreignObject
                         * scaling in a top-level browser window. WKWebView
                         * child surfaces give fixed descendants a viewport
                         * origin outside the foreignObject, leaving the caret
                         * and selection overlay at the top of Choro's canvas.
                         * Restore the normal in-foreignObject positioning only
                         * for this embedded Penpot surface.
                         */
                        g.text-editor > foreignObject > div {{
                            position: static !important;
                            transform: none !important;
                        }}
                    `;
                    (document.head || document.documentElement).appendChild(style);
                }};
                installEmbeddedTextEditorFix();"##,
                key_guard = penpot_embedded_key_guard_script()
            )
        } else {
            String::new()
        };
        format!(
            r##"(() => {{
                const assistantOpen = {assistant_open};
                const compareOpen = {compare_open};
                const surfaceId = {surface_id};
                const theme = {theme};
                window.__choroTheme = theme;
                {embedded_workarounds}
                try {{
                    window.sessionStorage.setItem("choro.assistant.open", String(assistantOpen));
                }} catch (_) {{}}
                window.__choroAssistantOpen = assistantOpen;
                window.__choroCompareOpen = compareOpen;
                const installCompareStyle = () => {{
                    const styleId = "choro-embedded-penpot-compare";
                    let style = document.getElementById(styleId);
                    if (!style) {{
                        style = document.createElement("style");
                        style.id = styleId;
                        (document.head || document.documentElement).appendChild(style);
                    }}
                    const hiddenChrome = [];
                    if (window.__choroAssistantOpen || window.__choroCompareOpen) {{
                        hiddenChrome.push(
                            "#left-sidebar-aside, [data-testid='left-sidebar'] {{ display: none !important; }}"
                        );
                    }}
                    if (window.__choroCompareOpen) {{
                        hiddenChrome.push(
                            "#right-sidebar-aside, [data-testid='right-sidebar'] {{ display: none !important; }}"
                        );
                    }}
                    style.textContent = hiddenChrome.join("\n");
                }};
                installCompareStyle();
                let attempts = 0;
                const sync = () => {{
                    const api = window.choroPenpot;
                    if (api && typeof api.setCompareMode === "function") {{
                        api.setCompareMode(Boolean(window.__choroCompareOpen));
                    }}
                    if (api && typeof api.setAssistantOpen === "function") {{
                        api.setAssistantOpen(Boolean(
                            window.__choroAssistantOpen || window.__choroCompareOpen
                        ));
                    }}
                    if (api && typeof api.setTheme === "function") {{
                        api.setTheme(theme);
                    }}
                    if (api && (
                        typeof api.setCompareMode === "function" ||
                        typeof api.setAssistantOpen === "function"
                    ) && typeof api.setTheme === "function") {{
                        return;
                    }}
                    if (attempts++ < 240) window.setTimeout(sync, 250);
                }};
                sync();
                let lastMcpReport = "";
                let lastMcpConnectAttempt = 0;
                const activeFileId = (api) => {{
                    if (api && typeof api.getActiveFileId === "function") {{
                        const value = api.getActiveFileId();
                        if (value) return String(value);
                    }}
                    const fragment = window.location.hash || "";
                    const query = fragment.includes("?") ? fragment.split("?", 2)[1] : "";
                    return new URLSearchParams(query).get("file-id");
                }};
                const syncMcp = () => {{
                    const api = window.choroPenpot;
                    const now = Date.now();
                    if (
                        api &&
                        typeof api.ensureMcpConnected === "function" &&
                        now - lastMcpConnectAttempt > 3000
                    ) {{
                        lastMcpConnectAttempt = now;
                        try {{ api.ensureMcpConnected(); }} catch (_) {{}}
                    }}
                    let connected = false;
                    if (api && typeof api.isMcpConnected === "function") {{
                        try {{ connected = Boolean(api.isMcpConnected()); }} catch (_) {{}}
                    }}
                    const fileId = activeFileId(api);
                    const report = JSON.stringify({{
                        type: "mcpStatus",
                        connected,
                        fileId,
                        surfaceId,
                    }});
                    if (report !== lastMcpReport) {{
                        lastMcpReport = report;
                        try {{ window.ipc?.postMessage(report); }} catch (_) {{}}
                    }}
                    window.setTimeout(syncMcp, 500);
                }};
                syncMcp();
            }})();"##,
            embedded_workarounds = embedded_workarounds
        )
    }

    fn penpot_assistant_initialization_script(
        assistant_open: bool,
        compare_open: bool,
        surface_id: Uuid,
        theme: &super::PenpotTheme,
    ) -> String {
        penpot_initialization_script(assistant_open, compare_open, surface_id, theme, true)
    }

    fn penpot_chromium_initialization_script(
        assistant_open: bool,
        compare_open: bool,
        surface_id: Uuid,
        theme: &super::PenpotTheme,
    ) -> String {
        let bridge = format!(
            r##"(() => {{
                if (!window.ipc || window.ipc.__choroEngine !== "chromium") {{
                    const bridge = Object.freeze({{
                        __choroEngine: "chromium",
                        postMessage(value) {{
                            console.debug({prefix} + String(value));
                        }},
                    }});
                    try {{
                        Object.defineProperty(window, "ipc", {{
                            configurable: true,
                            value: bridge,
                        }});
                    }} catch (_) {{
                        window.ipc = bridge;
                    }}
                }}
            }})();"##,
            prefix = serde_json::to_string(CHROMIUM_IPC_PREFIX)
                .expect("static Chromium IPC prefix is valid JSON")
        );
        format!(
            "{bridge}\n{}",
            penpot_initialization_script(assistant_open, compare_open, surface_id, theme, false,)
        )
    }

    fn penpot_sidebar_tab_script(tab: PenpotSidebarTab) -> String {
        let tab = serde_json::to_string(tab.as_str()).expect("static tab name is valid JSON");
        format!(
            r#"(() => {{
                const tab = {tab};
                let attempts = 0;
                const select = () => {{
                    const api = window.choroPenpot;
                    if (api && typeof api.setSidebarTab === "function") {{
                        api.setSidebarTab(tab);
                        return;
                    }}
                    if (attempts++ < 240) window.setTimeout(select, 250);
                }};
                select();
            }})();"#
        )
    }

    fn penpot_left_sidebar_collapse_script() -> &'static str {
        r#"(() => {
            let attempts = 0;
            const collapse = () => {
                const api = window.choroPenpot;
                if (api && typeof api.collapseLeftSidebar === "function") {
                    api.collapseLeftSidebar();
                    return;
                }
                if (attempts++ < 240) window.setTimeout(collapse, 250);
            };
            collapse();
        })();"#
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

    fn sync_active_penpot_theme(active: &Active, next: &WebPreviewIntent) {
        let (
            WebPreviewIntent::PenpotUrl {
                theme: current_theme,
                ..
            },
            WebPreviewIntent::PenpotUrl { theme, .. },
        ) = (&active.intent, next)
        else {
            return;
        };
        if current_theme != theme {
            let _ = active
                .webview
                .evaluate_script(&penpot_theme_sync_script(theme));
        }
    }

    fn build(
        intent: &WebPreviewIntent,
        bounds: Bounds<Pixels>,
        window: &Window,
        messages: Rc<RefCell<VecDeque<DocEditorMessage>>>,
        penpot_messages: Arc<Mutex<VecDeque<PenpotMessage>>>,
        preview_messages: Rc<RefCell<VecDeque<ProjectPreviewMessage>>>,
        project_preview_live_urls: Rc<RefCell<HashMap<ProjectId, String>>>,
        app: AsyncApp,
        penpot_assistant_open: bool,
        penpot_compare_open: bool,
        penpot_surface_id: Option<Uuid>,
    ) -> Result<WebSurface, String> {
        let rect = to_rect(bounds);
        match intent {
            WebPreviewIntent::Url(url) => WebViewBuilder::new()
                .with_url(url)
                .with_bounds(rect)
                .with_transparent(false)
                .with_accept_first_mouse(true)
                .build_as_child(window)
                .map(WebSurface::WebKit)
                .map_err(|error| error.to_string()),
            WebPreviewIntent::PenpotUrl { url, theme } => {
                let surface_id = penpot_surface_id
                    .ok_or_else(|| "Design surface identity is missing".to_string())?;
                let chromium_requested = !std::env::var("CHORO_DESIGN_ENGINE")
                    .is_ok_and(|engine| engine.eq_ignore_ascii_case("webkit"));
                if chromium_requested && crate::chromium::is_ready() {
                    let chromium_script = penpot_chromium_initialization_script(
                        penpot_assistant_open,
                        penpot_compare_open,
                        surface_id,
                        theme,
                    );
                    match ChromiumSurface::new(
                        url,
                        chromium_script,
                        bounds,
                        window,
                        penpot_messages.clone(),
                        app.clone(),
                    ) {
                        Ok(surface) => return Ok(WebSurface::Chromium(surface)),
                        Err(error) => {
                            eprintln!("Chromium Design surface unavailable; using WebKit: {error}");
                        }
                    }
                }
                let script = penpot_assistant_initialization_script(
                    penpot_assistant_open,
                    penpot_compare_open,
                    surface_id,
                    theme,
                );
                let ipc_messages = penpot_messages.clone();
                let ipc_app = app.clone();
                let download_paths =
                    Arc::new(Mutex::new(HashMap::<String, VecDeque<PathBuf>>::new()));
                let started_download_paths = download_paths.clone();
                let started_messages = penpot_messages.clone();
                let started_app = app.clone();
                let completed_download_paths = download_paths;
                let completed_messages = penpot_messages.clone();
                let completed_app = app.clone();
                WebViewBuilder::new()
                    .with_initialization_script(script)
                    .with_ipc_handler(move |request| {
                        if request.body().len() > MAX_PENPOT_MESSAGE_BYTES {
                            eprintln!("Design WebKit message exceeded the size limit");
                            return;
                        }
                        match serde_json::from_str::<PenpotMessage>(request.body()) {
                            Ok(message) => {
                                if let Ok(mut messages) = ipc_messages.lock() {
                                    messages.push_back(message);
                                }
                                let _ = ipc_app.refresh();
                            }
                            Err(error) => {
                                eprintln!("invalid Design WebKit message: {error}");
                            }
                        }
                    })
                    .with_download_started_handler(move |uri, suggested_path| {
                        let target = match design_download_destination(suggested_path) {
                            Ok(target) => target,
                            Err(error) => {
                                eprintln!("could not prepare Design export download: {error}");
                                if let Ok(mut messages) = started_messages.lock() {
                                    messages.push_back(PenpotMessage::ExportFinished {
                                        success: false,
                                        file_name: design_export_file_name(suggested_path),
                                    });
                                }
                                let _ = started_app.refresh();
                                return false;
                            }
                        };

                        if let Ok(mut paths) = started_download_paths.lock() {
                            paths.entry(uri).or_default().push_back(target.clone());
                        }
                        *suggested_path = target;
                        true
                    })
                    .with_download_completed_handler(move |uri, _, success| {
                        let target = completed_download_paths.lock().ok().and_then(|mut paths| {
                            let queue = paths.get_mut(&uri)?;
                            let target = queue.pop_front();
                            if queue.is_empty() {
                                paths.remove(&uri);
                            }
                            target
                        });
                        let file_name = target
                            .as_deref()
                            .map(design_export_file_name)
                            .unwrap_or_else(|| "design export".to_string());

                        if success {
                            if let Some(target) = target.as_deref() {
                                eprintln!("Design export saved to {}", target.display());
                            } else {
                                eprintln!("Design export finished: {file_name}");
                            }
                        } else {
                            eprintln!("Design export failed: {file_name}");
                        }
                        if let Ok(mut messages) = completed_messages.lock() {
                            messages
                                .push_back(PenpotMessage::ExportFinished { success, file_name });
                        }
                        let _ = completed_app.refresh();
                    })
                    .with_url(url)
                    .with_bounds(rect)
                    .with_transparent(false)
                    .with_accept_first_mouse(true)
                    .build_as_child(window)
                    .map(WebSurface::WebKit)
                    .map_err(|error| error.to_string())
            }
            WebPreviewIntent::ProjectPreview {
                project_id, url, ..
            } => {
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
                ProjectPreviewMessageHandler::install(
                    webview.manager(),
                    *project_id,
                    preview_messages.clone(),
                    app.clone(),
                    main_thread,
                );
                ProjectPreviewConsoleMessageHandler::install(
                    webview.manager(),
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
                Ok(WebSurface::WebKit(webview))
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
                    .map(WebSurface::WebKit)
                    .map_err(|error| error.to_string())
            }
            WebPreviewIntent::Visualization { path, theme, .. } => {
                let html =
                    visualization_document(path, theme).map_err(|error| error.to_string())?;
                WebViewBuilder::new()
                    .with_html(html)
                    .with_bounds(rect)
                    .with_transparent(false)
                    .with_navigation_handler(|url| {
                        url == "about:blank" || url.starts_with("data:text/html")
                    })
                    .build_as_child(window)
                    .map(WebSurface::WebKit)
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

        use super::{
            bind_project_preview_message, decode_project_preview_console_entry,
            decode_project_preview_message, design_export_file_name, inject_visualization_chrome,
            penpot_assistant_initialization_script, penpot_chrome_sync_script,
            penpot_chromium_initialization_script, penpot_left_sidebar_collapse_script,
            penpot_message_matches_surface, penpot_sidebar_tab_script, penpot_theme_sync_script,
            preview_key_event_data, project_preview_message_live_url, same_surface,
            surface_visible, unique_design_download_path, PenpotMessage, PenpotSidebarTab,
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
            PenpotTheme, ProjectPreviewConsoleLevel, ProjectPreviewMessage, WebPreviewIntent,
        };

        fn test_penpot_theme() -> PenpotTheme {
            PenpotTheme {
                name: "Test".into(),
                dark: true,
                sink: "#111111".into(),
                nav: "#121212".into(),
                base: "#131313".into(),
                surface: "#202020".into(),
                surface_2: "#242424".into(),
                focus: "#282828".into(),
                text_1: "#f5f5f5".into(),
                text_2: "#d0d0d0".into(),
                text_3: "#999999".into(),
                text_4: "#707070".into(),
                line: "#303030".into(),
                line_2: "#383838".into(),
                accent: "#cac9ee".into(),
                accent_2: "#d9d8f6".into(),
                on_accent: "#202027".into(),
                accent_soft: "#303049".into(),
                accent_line: "#555577".into(),
                info: "#85b8df".into(),
                overlay: "rgb(0 0 0 / 72%)".into(),
                shadow: "rgb(0 0 0 / 60%)".into(),
            }
        }

        #[test]
        fn web_surface_stays_hidden_until_content_is_ready() {
            assert!(!surface_visible(false, false));
            assert!(surface_visible(false, true));
            assert!(!surface_visible(true, true));
        }

        #[test]
        fn design_export_uses_only_the_suggested_file_name() {
            assert_eq!(
                design_export_file_name(std::path::Path::new("../../Coffee home.svg")),
                "Coffee home.svg"
            );
            assert_eq!(
                design_export_file_name(std::path::Path::new("")),
                "design-export"
            );
        }

        #[test]
        fn design_export_does_not_overwrite_an_existing_download() {
            let directory = tempfile::tempdir().expect("temporary export directory");
            std::fs::write(directory.path().join("Coffee home.png"), b"existing")
                .expect("existing export");
            std::fs::write(directory.path().join("Coffee home (1).png"), b"existing")
                .expect("second existing export");

            assert_eq!(
                unique_design_download_path(directory.path(), "Coffee home.png"),
                directory.path().join("Coffee home (2).png")
            );
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
        fn penpot_theme_changes_reuse_the_open_design_surface() {
            let first = WebPreviewIntent::PenpotUrl {
                url: "https://design.example/workspace#file=1".into(),
                theme: test_penpot_theme(),
            };
            let mut next_theme = test_penpot_theme();
            next_theme.name = "Light".into();
            next_theme.dark = false;
            next_theme.base = "#ffffff".into();
            let second = WebPreviewIntent::PenpotUrl {
                url: "https://design.example/workspace#file=1".into(),
                theme: next_theme,
            };

            assert!(same_surface(&first, &second));
        }

        #[test]
        fn penpot_theme_sync_uses_the_stable_integration_api() {
            let script = penpot_theme_sync_script(&test_penpot_theme());

            assert!(script.contains("window.__choroTheme = theme"));
            assert!(script.contains("api.setTheme(theme)"));
            assert!(script.contains("\"accent\":\"#cac9ee\""));
        }

        #[test]
        fn penpot_chrome_sync_uses_the_stable_integration_api() {
            let open = penpot_chrome_sync_script(true, false);
            let closed = penpot_chrome_sync_script(false, false);

            assert!(open.contains("const assistantOpen = true"));
            assert!(closed.contains("const assistantOpen = false"));
            assert!(open.contains("api.setAssistantOpen"));
            assert!(open.contains("if (assistantOpen || compareOpen)"));
            assert!(open.contains("#left-sidebar-aside"));
            assert!(!open.contains("querySelector"));
            assert!(!open.contains("style.width"));
        }

        #[test]
        fn penpot_assistant_state_follows_choro_after_webview_navigation() {
            let script = penpot_assistant_initialization_script(
                false,
                false,
                Uuid::from_u128(1),
                &test_penpot_theme(),
            );

            assert!(script.contains("const assistantOpen = false"));
            assert!(script.contains("const compareOpen = false"));
            assert!(script.contains("sessionStorage.setItem"));
            assert!(!script.contains("sessionStorage.getItem"));
            assert!(script.contains("api.setAssistantOpen"));
            assert!(
                script.contains("if (window.__choroAssistantOpen || window.__choroCompareOpen)")
            );
            assert!(script.contains("#left-sidebar-aside"));
            assert!(!script.contains("querySelector"));
            assert!(script.contains("choro-embedded-penpot-fixes"));
            assert!(script.contains("g.text-editor > foreignObject > div"));
            assert!(script.contains("position: static !important"));
            assert!(script.contains("transform: none !important"));
            assert!(script.contains("api.ensureMcpConnected"));
            assert!(script.contains("api.isMcpConnected"));
            assert!(script.contains(r#"type: "mcpStatus""#));
            assert!(script.contains("api.setTheme(theme)"));
            assert!(script.contains(r#"window.__choroTheme = theme"#));
        }

        #[test]
        fn chromium_design_uses_its_console_bridge_without_webkit_workarounds() {
            let script = penpot_chromium_initialization_script(
                false,
                false,
                Uuid::from_u128(1),
                &test_penpot_theme(),
            );

            assert!(script.contains("__CHORO_DESIGN_IPC__"));
            assert!(script.contains("window.ipc"));
            assert!(!script.contains("choro-embedded-penpot-fixes"));
            assert!(script.contains("api.ensureMcpConnected"));
        }

        #[test]
        fn penpot_agent_tab_can_only_request_the_existing_assistant() {
            let message = serde_json::from_str::<PenpotMessage>(r#"{"type":"openAssistant"}"#)
                .expect("valid Design integration message");

            assert_eq!(message, PenpotMessage::OpenAssistant);
            assert!(serde_json::from_str::<PenpotMessage>(r#"{"type":"runCommand"}"#).is_err());
        }

        #[test]
        fn penpot_reports_exact_remote_file_readiness() {
            let file_id = Uuid::new_v4();
            let surface_id = Uuid::new_v4();
            let message = serde_json::from_value::<PenpotMessage>(serde_json::json!({
                "type": "mcpStatus",
                "connected": true,
                "fileId": file_id,
                "surfaceId": surface_id,
            }))
            .expect("valid Design MCP readiness message");

            assert_eq!(
                message,
                PenpotMessage::McpStatus {
                    connected: true,
                    file_id: Some(file_id),
                    surface_id,
                }
            );
        }

        #[test]
        fn stale_penpot_surface_status_is_rejected_after_navigation() {
            let active_surface_id = Uuid::new_v4();
            let stale = PenpotMessage::McpStatus {
                connected: true,
                file_id: Some(Uuid::new_v4()),
                surface_id: Uuid::new_v4(),
            };
            let current = PenpotMessage::McpStatus {
                connected: true,
                file_id: Some(Uuid::new_v4()),
                surface_id: active_surface_id,
            };

            assert!(!penpot_message_matches_surface(
                &stale,
                Some(active_surface_id)
            ));
            assert!(penpot_message_matches_surface(
                &current,
                Some(active_surface_id)
            ));
        }

        #[test]
        fn native_assistant_tabs_use_the_stable_penpot_api() {
            let script = penpot_sidebar_tab_script(PenpotSidebarTab::Assets);

            assert!(script.contains(r#"const tab = "assets""#));
            assert!(script.contains("api.setSidebarTab"));
            assert!(!script.contains("querySelector"));
        }

        #[test]
        fn native_assistant_collapse_uses_the_stable_penpot_api() {
            let script = penpot_left_sidebar_collapse_script();

            assert!(script.contains("api.collapseLeftSidebar"));
            assert!(!script.contains("querySelector"));
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use std::path::Path;

    use gpui::{AsyncApp, Bounds, Pixels, Window};

    use super::{
        DocEditorMessage, PenpotMessage, PenpotSidebarTab, ProjectPreviewMessage, WebPreviewIntent,
    };

    pub struct WebPreviewHost;

    impl WebPreviewHost {
        pub fn new(_app: AsyncApp) -> Self {
            Self
        }
        pub fn set_intent(&mut self, _intent: Option<WebPreviewIntent>) -> bool {
            false
        }
        pub fn set_suspended(&mut self, _suspended: bool) {}
        pub fn set_overlay_suspended(&mut self, _suspended: bool) {}
        pub fn place(&mut self, _bounds: Bounds<Pixels>, _window: &Window) {}
        pub fn take_doc_editor_messages(&mut self) -> Vec<DocEditorMessage> {
            Vec::new()
        }
        pub fn take_project_preview_messages(&mut self) -> Vec<ProjectPreviewMessage> {
            Vec::new()
        }
        pub fn take_penpot_messages(&mut self) -> Vec<PenpotMessage> {
            Vec::new()
        }
        pub fn navigate_url(&self, _url: &str) -> Result<(), String> {
            Err("Embedded Design is only available on macOS.".to_string())
        }
        pub fn set_penpot_assistant_open(&mut self, _open: bool) {}
        pub fn set_penpot_compare_open(&mut self, _open: bool) {}
        pub fn set_penpot_keepalive(&mut self, _keepalive: bool) {}
        pub fn select_penpot_sidebar_tab(&self, _tab: PenpotSidebarTab) {}
        pub fn collapse_penpot_left_sidebar(&self) {}
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
