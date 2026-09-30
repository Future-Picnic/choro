use std::ffi::{c_char, c_int, c_void, CStr};
use std::ptr::NonNull;

use anyhow::Result;

use super::BridgeEvent;

unsafe extern "C" {
    fn choro_sparkle_destroy(bridge: *mut c_void);
    fn choro_sparkle_check(bridge: *mut c_void);
    fn choro_sparkle_download(bridge: *mut c_void) -> i8;
    fn choro_sparkle_dismiss_offer(bridge: *mut c_void) -> i8;
    fn choro_sparkle_cancel_download(bridge: *mut c_void) -> i8;
    fn choro_sparkle_reply_ready(bridge: *mut c_void, choice: isize) -> i8;
    fn choro_bundle_installation_status() -> isize;
    fn choro_bundle_short_version() -> *mut c_char;
    fn choro_bundle_path() -> *mut c_char;
    fn choro_sparkle_free_string(value: *mut c_char);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReadyReply {
    NotNow,
    Install,
}

fn sparkle_choice(reply: ReadyReply) -> isize {
    // SPUUserUpdateChoiceSkip = 0 and SPUUserUpdateChoiceInstall = 1 in
    // Sparkle 2.9.6's SPUUserUpdateState.h.
    match reply {
        ReadyReply::NotNow => 0,
        ReadyReply::Install => 1,
    }
}

struct CallbackContext {
    events: async_channel::Sender<BridgeEvent>,
}

pub(super) struct NativeUpdater {
    bridge: NonNull<c_void>,
    _callback_context: Box<CallbackContext>,
}

impl NativeUpdater {
    pub(super) fn start(
        credential: ide_core::git::ChoroReleaseCredential,
        manual_start: bool,
    ) -> Result<(Self, async_channel::Receiver<BridgeEvent>)> {
        let feed_override = std::env::var("CHORO_UPDATE_FEED_URL").unwrap_or_default();
        let (sender, receiver) = async_channel::unbounded();
        let mut callback_context = Box::new(CallbackContext { events: sender });
        let context = (&mut *callback_context) as *mut CallbackContext as *mut c_void;
        let bridge = unsafe {
            ide_core::git::start_authenticated_choro_release_updater(
                credential,
                &feed_override,
                native_callback,
                context,
                manual_start,
            )
        }?;
        Ok((
            Self {
                bridge,
                _callback_context: callback_context,
            },
            receiver,
        ))
    }

    pub(super) fn check(&self) {
        unsafe { choro_sparkle_check(self.bridge.as_ptr()) };
    }

    pub(super) fn download(&self) -> bool {
        unsafe { choro_sparkle_download(self.bridge.as_ptr()) != 0 }
    }

    pub(super) fn dismiss_offer(&self) -> bool {
        unsafe { choro_sparkle_dismiss_offer(self.bridge.as_ptr()) != 0 }
    }

    pub(super) fn cancel_download(&self) -> bool {
        unsafe { choro_sparkle_cancel_download(self.bridge.as_ptr()) != 0 }
    }

    pub(super) fn cancel_ready(&self) -> bool {
        self.reply_ready(ReadyReply::NotNow)
    }

    pub(super) fn install_ready(&self) -> bool {
        self.reply_ready(ReadyReply::Install)
    }

    fn reply_ready(&self, reply: ReadyReply) -> bool {
        unsafe { choro_sparkle_reply_ready(self.bridge.as_ptr(), sparkle_choice(reply)) != 0 }
    }
}

impl Drop for NativeUpdater {
    fn drop(&mut self) {
        unsafe { choro_sparkle_destroy(self.bridge.as_ptr()) };
    }
}

pub(super) fn bundle_short_version() -> String {
    take_native_string(unsafe { choro_bundle_short_version() })
        .unwrap_or_else(|| "Development".to_string())
}

pub(super) fn bundle_path() -> String {
    take_native_string(unsafe { choro_bundle_path() }).unwrap_or_default()
}

pub(super) fn installation_error() -> Option<String> {
    installation_error_for_status(unsafe { choro_bundle_installation_status() })
}

fn installation_error_for_status(status: isize) -> Option<String> {
    let reason = match status {
        0 => return None,
        1 => "Choro is running from a translocated app location",
        2 => "Choro is running from a read-only volume",
        3 => "This copy of Choro is not running from an application bundle",
        4 => "This copy of Choro cannot be replaced at its current location",
        _ => "Choro could not verify that its current location is updatable",
    };
    Some(format!(
        "{reason}. Move Choro to Applications, reopen it, and check for updates again."
    ))
}

unsafe extern "C" fn native_callback(
    context: *mut c_void,
    event: c_int,
    primary: *const c_char,
    secondary: *const c_char,
    value: u64,
) {
    if context.is_null() {
        return;
    }
    let context = unsafe { &*(context as *const CallbackContext) };
    let event = BridgeEvent::from_native(
        event,
        borrowed_string(primary),
        borrowed_string(secondary),
        value,
    );
    if let Some(event) = event {
        let _ = context.events.try_send(event);
    }
}

fn borrowed_string(value: *const c_char) -> String {
    if value.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(value) }
        .to_string_lossy()
        .into_owned()
}

fn take_native_string(value: *mut c_char) -> Option<String> {
    if value.is_null() {
        return None;
    }
    let string = unsafe { CStr::from_ptr(value) }
        .to_string_lossy()
        .into_owned();
    unsafe { choro_sparkle_free_string(value) };
    Some(string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_now_maps_to_sparkles_skip_choice() {
        assert_eq!(sparkle_choice(ReadyReply::NotNow), 0);
        assert_eq!(sparkle_choice(ReadyReply::Install), 1);
    }

    #[test]
    fn non_updatable_locations_have_actionable_guidance() {
        for status in 1..=4 {
            let message = installation_error_for_status(status).unwrap();
            assert!(message.contains("Move Choro to Applications"));
        }
        assert_eq!(installation_error_for_status(0), None);
    }
}
