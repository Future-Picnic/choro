//! Route macOS Dock activation and Quit through the workspace lifecycle.

use std::cell::Cell;

use gpui::Window;
use objc::{
    class, msg_send,
    runtime::{self, Object, Sel, BOOL, NO, YES},
    sel, sel_impl,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

thread_local! {
    static ALLOW_TERMINATION: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn allow_termination() {
    ALLOW_TERMINATION.with(|allowed| allowed.set(true));
}

pub(crate) fn install(main_window: &Window) -> anyhow::Result<()> {
    let handle = HasWindowHandle::window_handle(main_window)?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        anyhow::bail!("main window has no AppKit handle");
    };

    unsafe {
        let view = handle.ns_view.as_ptr() as *mut Object;
        let window: *mut Object = msg_send![view, window];
        anyhow::ensure!(!window.is_null(), "main window has no NSWindow");
        let app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
        let delegate: *mut Object = msg_send![app, delegate];
        anyhow::ensure!(!delegate.is_null(), "application has no delegate");
        let method = runtime::class_getInstanceMethod(
            (*delegate).class(),
            sel!(applicationShouldHandleReopen:hasVisibleWindows:),
        );
        anyhow::ensure!(!method.is_null(), "application has no reopen handler");

        // Window-server numbers can change when a window is ordered out. An
        // identifier survives hiding/minimizing without retaining a raw pointer.
        let identifier: *mut Object = msg_send![class!(NSString), stringWithUTF8String:c"com.ritmus.choro.workspace".as_ptr()];
        let _: () = msg_send![window, setIdentifier:identifier];

        // GPUI 0.2.2 only calls Application::on_reopen when AppKit reports no
        // visible windows. A visible companion prevents that callback entirely.
        // Replace this one delegate method; all other GPUI lifecycle handling stays intact.
        runtime::method_setImplementation(
            method.cast_mut(),
            std::mem::transmute::<*const (), runtime::Imp>(handle_reopen as *const ()),
        );

        // Dock Quit sends terminate: directly and bypasses our menu action.
        // Cancel that first request and let the workspace finish saving before
        // complete_shutdown explicitly permits the final termination.
        anyhow::ensure!(
            runtime::class_addMethod(
                (*delegate).class() as *const _ as *mut _,
                sel!(applicationShouldTerminate:),
                std::mem::transmute::<*const (), runtime::Imp>(handle_terminate as *const ()),
                c"Q@:@".as_ptr(),
            ) == YES,
            "application already has a termination handler"
        );
    }
    Ok(())
}

extern "C" fn handle_reopen(
    _delegate: &Object,
    _selector: Sel,
    app: *mut Object,
    _has_visible_windows: BOOL,
) -> BOOL {
    unsafe {
        let window = workspace_window(app);
        if window.is_null() {
            return YES;
        }
        show_workspace(app, window);
    }
    // We selected the workspace explicitly; AppKit should not pick another window.
    NO
}

unsafe fn workspace_window(app: *mut Object) -> *mut Object {
    if app.is_null() {
        return std::ptr::null_mut();
    }
    let identifier: *mut Object =
        msg_send![class!(NSString), stringWithUTF8String:c"com.ritmus.choro.workspace".as_ptr()];
    let windows: *mut Object = msg_send![app, windows];
    let count: usize = msg_send![windows, count];
    for index in 0..count {
        let window: *mut Object = msg_send![windows, objectAtIndex:index];
        let candidate: *mut Object = msg_send![window, identifier];
        let matches: BOOL = msg_send![candidate, isEqualToString:identifier];
        if matches == YES {
            return window;
        }
    }
    std::ptr::null_mut()
}

unsafe fn show_workspace(app: *mut Object, window: *mut Object) {
    let minimized: BOOL = msg_send![window, isMiniaturized];
    if minimized == YES {
        let _: () = msg_send![window, deminiaturize:app];
    }
    let _: () = msg_send![app, activateIgnoringOtherApps:YES];
    let _: () = msg_send![window, makeKeyAndOrderFront:app];
}

extern "C" fn handle_terminate(_delegate: &Object, _selector: Sel, app: *mut Object) -> usize {
    if ALLOW_TERMINATION.with(Cell::get) {
        return 1; // NSTerminateNow
    }
    unsafe {
        let window = workspace_window(app);
        if window.is_null() {
            return 1;
        }
        show_workspace(app, window);
        // Invokes GPUI's on_window_should_close handler, including its save
        // failure UI. It keeps the window alive until shutdown is Ready.
        let _: () = msg_send![window, performClose:app];
    }
    0 // NSTerminateCancel
}
