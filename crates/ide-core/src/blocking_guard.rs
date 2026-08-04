//! Debug-build tripwire for blocking work on the UI thread.
//!
//! The app marks its UI thread once at startup; blocking helpers (subprocess
//! spawns, synchronous file reads) call [`debug_warn_if_ui_thread`] so a call
//! that sneaks into a render path is loud in development instead of showing up
//! as mysterious scroll jank.

use std::sync::OnceLock;
use std::thread::ThreadId;

static UI_THREAD: OnceLock<ThreadId> = OnceLock::new();

/// Record the calling thread as the UI thread. Call once from `main` before
/// the app starts rendering; later calls are ignored.
pub fn mark_ui_thread() {
    let _ = UI_THREAD.set(std::thread::current().id());
}

/// In debug builds, print a warning when called on the marked UI thread.
/// Release builds compile this to nothing.
pub fn debug_warn_if_ui_thread(operation: &str) {
    #[cfg(debug_assertions)]
    {
        if UI_THREAD.get() == Some(&std::thread::current().id()) {
            eprintln!(
                "warning: blocking call `{operation}` on the UI thread — move it to the \
                 background executor or render from cached state"
            );
        }
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = operation;
    }
}
