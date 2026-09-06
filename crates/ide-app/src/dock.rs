//! Route macOS Dock clicks to the workspace, even while the companion is visible.

use std::cell::Cell;

use gpui::Window;
use objc::{
    class, msg_send,
    runtime::{self, Object, Sel, BOOL, NO, YES},
    sel, sel_impl,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

thread_local! {
    // Resolve the window through AppKit on each click instead of retaining a raw
    // NSWindow pointer beyond its lifetime. All access is on the main thread.
    static MAIN_WINDOW_NUMBER: Cell<isize> = const { Cell::new(0) };
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

        let number: isize = msg_send![window, windowNumber];
        MAIN_WINDOW_NUMBER.with(|stored| stored.set(number));

        // GPUI 0.2.2 only calls Application::on_reopen when AppKit reports no
        // visible windows. A visible companion prevents that callback entirely.
        // Replace this one delegate method; all other GPUI lifecycle handling stays intact.
        runtime::method_setImplementation(
            method.cast_mut(),
            std::mem::transmute::<*const (), runtime::Imp>(handle_reopen as *const ()),
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
    let number = MAIN_WINDOW_NUMBER.with(Cell::get);
    if number == 0 || app.is_null() {
        return YES;
    }
    unsafe {
        let window: *mut Object = msg_send![app, windowWithWindowNumber:number];
        if window.is_null() {
            return YES;
        }
        let minimized: BOOL = msg_send![window, isMiniaturized];
        if minimized == YES {
            let _: () = msg_send![window, deminiaturize:app];
        }
        let _: () = msg_send![app, activateIgnoringOtherApps:YES];
        let _: () = msg_send![window, makeKeyAndOrderFront:app];
    }
    // We selected the workspace explicitly; AppKit should not pick another window.
    NO
}
