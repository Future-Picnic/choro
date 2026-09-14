//! Process-wide Chromium Embedded Framework lifecycle for the Design surface.
//!
//! CEF must be loaded and initialized before GPUI creates `NSApplication`, and
//! it must share the application's Cocoa event loop. The actual browser child
//! is owned by `ui::center::web_preview`; this module only owns the framework,
//! helper-process configuration, external message pump, and orderly shutdown.

use std::cell::Cell;
use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use cef::*;
use objc::runtime::{
    self as objc_runtime, Class as ObjcClass, Object as ObjcObject, Protocol as ObjcProtocol,
    Sel as ObjcSel, BOOL as ObjcBool, NO, YES,
};
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol};
use objc2::{define_class, msg_send, sel, AnyThread, DefinedClass};
use objc2_app_kit::NSEventTrackingRunLoopMode;
use objc2_foundation::{
    NSNumber, NSObjectNSThreadPerformAdditions, NSRunLoop, NSRunLoopCommonModes, NSThread, NSTimer,
};

const CEF_FRAMEWORK: &str = "Chromium Embedded Framework.framework/Chromium Embedded Framework";
const CEF_HELPER_APP: &str = "choro Helper.app/Contents/MacOS/choro Helper";
const TIMER_DELAY_PLACEHOLDER: i64 = i32::MAX as i64;
const MAX_TIMER_DELAY_MS: i64 = 1000 / 30;

static READY: AtomicBool = AtomicBool::new(false);
static PENDING_BROWSERS: AtomicUsize = AtomicUsize::new(0);
static OPEN_BROWSERS: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    static HANDLING_SEND_EVENT: Cell<bool> = const { Cell::new(false) };
    static BROWSERS: std::cell::RefCell<Vec<Browser>> = const {
        std::cell::RefCell::new(Vec::new())
    };
}

/// Add CEF's event-dispatch contract directly to GPUI's registered application
/// class before either framework requests the shared application singleton.
/// Creating a separate NSApplication subclass breaks GPUI because its native
/// callbacks require the `platform` ivar declared by `GPUIApplication`.
fn setup_application() -> Result<(), String> {
    let app_class = ObjcClass::get("GPUIApplication")
        .ok_or_else(|| "GPUI's macOS application class was not registered".to_string())?;
    let app_class_ptr = app_class as *const ObjcClass as *mut ObjcClass;

    let cr_app =
        ensure_application_protocol("CrAppProtocol", None, &[("isHandlingSendEvent", b"c@:\0")])?;
    let cr_app_control = ensure_application_protocol(
        "CrAppControlProtocol",
        Some(cr_app),
        &[("setHandlingSendEvent:", b"v@:c\0")],
    )?;
    let cef_app = ensure_application_protocol("CefAppProtocol", Some(cr_app_control), &[])?;

    for (protocol_name, protocol) in [
        ("CrAppProtocol", cr_app),
        ("CrAppControlProtocol", cr_app_control),
        ("CefAppProtocol", cef_app),
    ] {
        let added = unsafe { objc_runtime::class_addProtocol(app_class_ptr, protocol) };
        if added == NO
            && unsafe { objc_runtime::class_conformsToProtocol(app_class, protocol) } == NO
        {
            return Err(format!(
                "could not attach Chromium protocol {protocol_name} to GPUI"
            ));
        }
    }

    add_application_method(
        app_class_ptr,
        ObjcSel::register("sendEvent:"),
        gpui_send_event as *const (),
        b"v@:@\0",
    )?;
    add_application_method(
        app_class_ptr,
        ObjcSel::register("setHandlingSendEvent:"),
        gpui_set_handling_send_event as *const (),
        b"v@:c\0",
    )?;
    add_application_method(
        app_class_ptr,
        ObjcSel::register("isHandlingSendEvent"),
        gpui_is_handling_send_event as *const (),
        b"c@:\0",
    )?;

    let app: *mut ObjcObject =
        unsafe { objc::__send_message(app_class, ObjcSel::register("sharedApplication"), ()) }
            .map_err(|error| format!("GPUI sharedApplication message failed: {error}"))?;
    if app.is_null() {
        return Err("GPUI could not create the shared macOS application".to_string());
    }
    Ok(())
}

fn ensure_application_protocol(
    name: &str,
    parent: Option<&ObjcProtocol>,
    methods: &[(&str, &'static [u8])],
) -> Result<&'static ObjcProtocol, String> {
    if let Some(protocol) = ObjcProtocol::get(name) {
        return Ok(protocol);
    }

    let name_c = CString::new(name)
        .map_err(|_| format!("Chromium protocol name {name:?} contained a null byte"))?;
    let protocol = unsafe { objc_runtime::objc_allocateProtocol(name_c.as_ptr()) };
    if protocol.is_null() {
        return ObjcProtocol::get(name)
            .ok_or_else(|| format!("could not allocate Chromium protocol {name}"));
    }
    if let Some(parent) = parent {
        unsafe { objc_runtime::protocol_addProtocol(protocol, parent) };
    }
    for (selector, encoding) in methods {
        unsafe {
            objc_runtime::protocol_addMethodDescription(
                protocol,
                ObjcSel::register(selector),
                encoding.as_ptr().cast(),
                YES,
                YES,
            );
        }
    }
    unsafe { objc_runtime::objc_registerProtocol(protocol) };
    ObjcProtocol::get(name).ok_or_else(|| format!("Chromium protocol {name} did not register"))
}

fn add_application_method(
    app_class: *mut ObjcClass,
    selector: ObjcSel,
    implementation: *const (),
    encoding: &'static [u8],
) -> Result<(), String> {
    let implementation =
        unsafe { std::mem::transmute::<*const (), objc_runtime::Imp>(implementation) };
    let added = unsafe {
        objc_runtime::class_addMethod(
            app_class,
            selector,
            implementation,
            encoding.as_ptr().cast(),
        )
    };
    if added == NO {
        return Err(format!(
            "could not attach Chromium selector {selector:?} to GPUI"
        ));
    }
    Ok(())
}

extern "C" fn gpui_send_event(this: &mut ObjcObject, _selector: ObjcSel, event: *mut ObjcObject) {
    let was_handling = HANDLING_SEND_EVENT.with(|handling| {
        let previous = handling.get();
        handling.set(true);
        previous
    });
    unsafe {
        let superclass = ObjcClass::get("GPUIApplication")
            .and_then(ObjcClass::superclass)
            .expect("GPUIApplication superclass");
        let result: Result<(), _> =
            objc::__send_super_message(this, superclass, ObjcSel::register("sendEvent:"), (event,));
        if let Err(error) = result {
            eprintln!("Chromium could not forward a macOS application event: {error}");
        }
    }
    if !was_handling {
        HANDLING_SEND_EVENT.with(|handling| handling.set(false));
    }
}

extern "C" fn gpui_set_handling_send_event(
    _this: &mut ObjcObject,
    _selector: ObjcSel,
    handling: ObjcBool,
) {
    HANDLING_SEND_EVENT.with(|value| value.set(handling != NO));
}

extern "C" fn gpui_is_handling_send_event(_this: &ObjcObject, _selector: ObjcSel) -> ObjcBool {
    HANDLING_SEND_EVENT.with(|handling| if handling.get() { YES } else { NO })
}

define_class! {
    #[unsafe(super(NSObject))]
    #[ivars = Weak<Mutex<ExternalPump>>]
    struct PumpEventHandler;

    impl PumpEventHandler {
        #[unsafe(method(scheduleWork:))]
        fn schedule_work(&self, delay_ms: &NSNumber) {
            let Ok(delay_ms) = i64::try_from(delay_ms.integerValue()) else {
                return;
            };
            let Some(pump) = self.ivars().upgrade() else {
                return;
            };
            ExternalPump::on_schedule_work(&pump, delay_ms);
        }

        #[unsafe(method(timerTimeout:))]
        fn timer_timeout(&self, _timer: &NSTimer) {
            let Some(pump) = self.ivars().upgrade() else {
                return;
            };
            ExternalPump::on_timer_timeout(&pump);
        }
    }

    unsafe impl NSObjectProtocol for PumpEventHandler {}
}

impl PumpEventHandler {
    fn new(pump: Weak<Mutex<ExternalPump>>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(pump);
        unsafe { msg_send![super(this), init] }
    }
}

struct PumpPlatform {
    owner_thread: Retained<NSThread>,
    timer: Option<Retained<NSTimer>>,
    event_handler: Retained<PumpEventHandler>,
}

// CEF may request work from a non-main thread. Cocoa objects are only touched
// by posting selectors back to `owner_thread`, matching CEF's macOS sample.
unsafe impl Send for PumpPlatform {}

impl PumpPlatform {
    fn new(pump: &Weak<Mutex<ExternalPump>>) -> Self {
        Self {
            owner_thread: NSThread::currentThread(),
            timer: None,
            event_handler: PumpEventHandler::new(pump.clone()),
        }
    }

    fn schedule(&self, delay_ms: i64) {
        let delay = isize::try_from(delay_ms).unwrap_or(isize::MAX);
        let number = NSNumber::numberWithInteger(delay);
        unsafe {
            self.event_handler
                .performSelector_onThread_withObject_waitUntilDone(
                    sel!(scheduleWork:),
                    &self.owner_thread,
                    Some(&number),
                    false,
                );
        }
    }

    fn set_timer(&mut self, delay_ms: i64) {
        let timer = unsafe {
            NSTimer::timerWithTimeInterval_target_selector_userInfo_repeats(
                delay_ms as f64 / 1000.0,
                &self.event_handler,
                sel!(timerTimeout:),
                None,
                false,
            )
        };
        let run_loop = NSRunLoop::currentRunLoop();
        unsafe {
            run_loop.addTimer_forMode(&timer, NSRunLoopCommonModes);
            run_loop.addTimer_forMode(&timer, NSEventTrackingRunLoopMode);
        }
        self.timer = Some(timer);
    }

    fn kill_timer(&mut self) {
        if let Some(timer) = self.timer.take() {
            timer.invalidate();
        }
    }
}

struct ExternalPump {
    active: bool,
    reentrancy_detected: bool,
    platform: PumpPlatform,
}

impl ExternalPump {
    fn new() -> Arc<Mutex<Self>> {
        Arc::new_cyclic(|weak| {
            Mutex::new(Self {
                active: false,
                reentrancy_detected: false,
                platform: PumpPlatform::new(weak),
            })
        })
    }

    fn schedule(&self, delay_ms: i64) {
        self.platform.schedule(delay_ms);
    }

    fn on_schedule_work(pump: &Arc<Mutex<Self>>, delay_ms: i64) {
        let run_now = {
            let Ok(mut pump) = pump.lock() else {
                return;
            };
            if delay_ms == TIMER_DELAY_PLACEHOLDER && pump.platform.timer.is_some() {
                return;
            }
            pump.platform.kill_timer();
            if delay_ms <= 0 {
                true
            } else {
                pump.platform
                    .set_timer(delay_ms.min(MAX_TIMER_DELAY_MS).max(1));
                false
            }
        };
        if run_now {
            Self::do_work(pump);
        }
    }

    fn on_timer_timeout(pump: &Arc<Mutex<Self>>) {
        let Ok(mut state) = pump.lock() else {
            return;
        };
        state.platform.kill_timer();
        drop(state);
        Self::do_work(pump);
    }

    fn do_work(pump: &Arc<Mutex<Self>>) {
        {
            let Ok(mut pump) = pump.lock() else {
                return;
            };
            if pump.active {
                pump.reentrancy_detected = true;
                return;
            }
            pump.reentrancy_detected = false;
            pump.active = true;
        }

        // CEF can synchronously request more work while processing this call.
        // Never hold the pump mutex across the framework boundary: the
        // callback must be able to enqueue its owner-thread selector, and a
        // nested Cocoa run loop must be able to record reentrancy.
        cef::do_message_loop_work();

        let next_delay = {
            let Ok(mut pump) = pump.lock() else {
                return;
            };
            pump.active = false;
            if pump.reentrancy_detected {
                Some(0)
            } else if pump.platform.timer.is_none() {
                Some(TIMER_DELAY_PLACEHOLDER)
            } else {
                None
            }
        };
        if let Some(delay_ms) = next_delay {
            if let Ok(pump) = pump.lock() {
                pump.schedule(delay_ms);
            }
        }
    }

    fn stop(&mut self) {
        self.platform.kill_timer();
    }
}

wrap_app! {
    struct ChoroCefApp {
        pump: Weak<Mutex<ExternalPump>>,
    }

    impl App {
        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(ChoroBrowserProcessHandler::new(self.pump.clone()))
        }
    }
}

wrap_browser_process_handler! {
    struct ChoroBrowserProcessHandler {
        pump: Weak<Mutex<ExternalPump>>,
    }

    impl BrowserProcessHandler {
        fn on_context_initialized(&self) {
            READY.store(true, Ordering::Release);
        }

        fn on_schedule_message_pump_work(&self, delay_ms: i64) {
            let Some(pump) = self.pump.upgrade() else {
                return;
            };
            if let Ok(pump) = pump.try_lock() {
                pump.schedule(delay_ms);
            };
        }
    }
}

struct FrameworkLoader {
    loaded: bool,
}

impl FrameworkLoader {
    fn load(path: &Path) -> Result<Self, String> {
        let path = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| "Chromium framework path contained a null byte".to_string())?;
        if cef::load_library(Some(unsafe { &*path.as_ptr() })) != 1 {
            return Err("CEF rejected the bundled Chromium framework".to_string());
        }
        Ok(Self { loaded: true })
    }
}

impl Drop for FrameworkLoader {
    fn drop(&mut self) {
        if self.loaded && cef::unload_library() != 1 {
            eprintln!("could not unload the Chromium framework cleanly");
        }
    }
}

pub struct Runtime {
    loader: Option<FrameworkLoader>,
    pump: Arc<Mutex<ExternalPump>>,
    initialized: bool,
}

impl Runtime {
    /// Load the bundled runtime and attach CEF to the existing Cocoa event loop.
    /// A missing runtime is not fatal: Design automatically falls back to
    /// WKWebView so development binaries remain launchable outside an app bundle.
    pub fn prepare() -> Result<Option<Self>, String> {
        if std::env::var("CHORO_DESIGN_ENGINE")
            .is_ok_and(|engine| engine.eq_ignore_ascii_case("webkit"))
        {
            return Ok(None);
        }

        let executable = std::env::current_exe()
            .map_err(|error| format!("could not locate the Choro executable: {error}"))?;
        let macos_dir = executable
            .parent()
            .ok_or_else(|| "Choro executable had no parent directory".to_string())?;
        let contents_dir = macos_dir
            .parent()
            .ok_or_else(|| "Choro executable was not inside a macOS app bundle".to_string())?;
        let framework_path = contents_dir.join("Frameworks").join(CEF_FRAMEWORK);
        let helper_path = contents_dir.join("Frameworks").join(CEF_HELPER_APP);

        if !framework_path.is_file() || !helper_path.is_file() {
            return Ok(None);
        }

        let loader = FrameworkLoader::load(&framework_path)?;
        let api_hash = cef::api_hash(cef::sys::CEF_API_VERSION_LAST, 0);
        if api_hash.is_null() {
            return Err("bundled Chromium framework does not match the Rust bindings".to_string());
        }

        let args = cef::args::Args::new();
        let process_result = cef::execute_process(
            Some(args.as_main_args()),
            None::<&mut App>,
            std::ptr::null_mut(),
        );
        if process_result >= 0 {
            return Err(format!(
                "the main Choro process was unexpectedly treated as a Chromium helper ({process_result})"
            ));
        }
        setup_application()?;

        // Demo and other explicitly isolated instances must not contend for
        // the production profile lock or inherit its cookies and site data.
        let cache_root = std::env::var_os("CHORO_DATA_DIR")
            .filter(|root| !root.is_empty())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                dirs::data_dir()
                    .unwrap_or_else(std::env::temp_dir)
                    .join(ide_core::APP_ID)
            })
            .join("chromium");
        let cache_path = cache_root.join("Default");
        std::fs::create_dir_all(&cache_path)
            .map_err(|error| format!("could not prepare Chromium cache: {error}"))?;

        let pump = ExternalPump::new();
        let mut app = ChoroCefApp::new(Arc::downgrade(&pump));
        let settings = Settings {
            no_sandbox: 0,
            external_message_pump: 1,
            cache_path: CefString::from(cache_path.to_string_lossy().as_ref()),
            root_cache_path: CefString::from(cache_root.to_string_lossy().as_ref()),
            persist_session_cookies: 1,
            log_severity: LogSeverity::WARNING,
            ..Default::default()
        };
        if cef::initialize(
            Some(args.as_main_args()),
            Some(&settings),
            Some(&mut app),
            std::ptr::null_mut(),
        ) != 1
        {
            return Err("CEF could not initialize the Chromium browser process".to_string());
        }

        if let Ok(pump) = pump.lock() {
            pump.schedule(0);
        }

        Ok(Some(Self {
            loader: Some(loader),
            pump,
            initialized: true,
        }))
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        if !self.initialized {
            return;
        }

        close_all_browsers();
        for _ in 0..200 {
            if browser_count() == 0 {
                break;
            }
            cef::do_message_loop_work();
            std::thread::sleep(Duration::from_millis(10));
        }

        if let Ok(mut pump) = self.pump.lock() {
            pump.stop();
        }
        READY.store(false, Ordering::Release);

        if browser_count() == 0 {
            cef::shutdown();
            self.initialized = false;
        } else {
            // CEF forbids shutdown/unload while a browser callback is still
            // alive. At process exit, leaking the loader is safer than tearing
            // executable pages out from under a late helper callback.
            eprintln!("Chromium still had a browser open during process shutdown");
            if let Some(loader) = self.loader.take() {
                std::mem::forget(loader);
            }
        }
    }
}

pub fn is_ready() -> bool {
    READY.load(Ordering::Acquire)
}

pub fn browser_creation_started() {
    PENDING_BROWSERS.fetch_add(1, Ordering::AcqRel);
}

pub fn browser_creation_failed() {
    decrement_pending_browsers();
}

pub fn register_browser(browser: &Browser) {
    decrement_pending_browsers();
    BROWSERS.with(|browsers| browsers.borrow_mut().push(browser.clone()));
    OPEN_BROWSERS.fetch_add(1, Ordering::AcqRel);
}

pub fn unregister_browser(browser: &mut Browser) {
    let removed = BROWSERS.with(|browsers| {
        let mut browsers = browsers.borrow_mut();
        let Some(index) = browsers
            .iter_mut()
            .position(|candidate| candidate.is_same(Some(browser)) != 0)
        else {
            return false;
        };
        browsers.remove(index);
        true
    });
    if removed {
        OPEN_BROWSERS.fetch_sub(1, Ordering::AcqRel);
    }
}

fn close_all_browsers() {
    let browsers = BROWSERS.with(|browsers| browsers.borrow().clone());
    for browser in browsers {
        if let Some(host) = browser.host() {
            host.close_browser(1);
        }
    }
}

fn decrement_pending_browsers() {
    let _ = PENDING_BROWSERS.fetch_update(Ordering::AcqRel, Ordering::Acquire, |pending| {
        pending.checked_sub(1)
    });
}

fn browser_count() -> usize {
    PENDING_BROWSERS.load(Ordering::Acquire) + OPEN_BROWSERS.load(Ordering::Acquire)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_paths_match_cef_macos_layout() {
        assert!(CEF_FRAMEWORK.ends_with("Chromium Embedded Framework"));
        assert!(CEF_HELPER_APP.ends_with("Contents/MacOS/choro Helper"));
    }
}
