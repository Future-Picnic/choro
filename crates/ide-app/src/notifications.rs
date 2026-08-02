#![allow(unexpected_cfgs)]

use std::process::Command;
#[cfg(target_os = "macos")]
use std::sync::OnceLock;

pub fn play_generated_sound() {
    let _ = Command::new("afplay")
        .arg("/System/Library/Sounds/Glass.aiff")
        .spawn();
}

#[cfg(target_os = "macos")]
fn ns_string(value: &str) -> *mut objc::runtime::Object {
    unsafe {
        use objc::{class, msg_send, sel, sel_impl};

        let string: *mut objc::runtime::Object = msg_send![class!(NSString), alloc];
        let string: *mut objc::runtime::Object =
            msg_send![string, initWithBytes:value.as_ptr() length:value.len() encoding:4usize];
        let string: *mut objc::runtime::Object = msg_send![string, autorelease];
        string
    }
}

#[cfg(target_os = "macos")]
fn install_notification_delegate() -> bool {
    static INSTALL: OnceLock<bool> = OnceLock::new();
    static mut DELEGATE: *mut objc::runtime::Object = std::ptr::null_mut();

    *INSTALL.get_or_init(|| unsafe {
        use objc::declare::ClassDecl;
        use objc::runtime::{Object, Sel, BOOL, YES};
        use objc::{class, msg_send, sel, sel_impl};

        extern "C" fn did_activate_notification(
            _delegate: &Object,
            _cmd: Sel,
            center: *mut Object,
            notification: *mut Object,
        ) {
            unsafe {
                let _: () = msg_send![center, removeDeliveredNotification:notification];
            }
        }

        extern "C" fn should_present_notification(
            _delegate: &Object,
            _cmd: Sel,
            _center: *mut Object,
            _notification: *mut Object,
        ) -> BOOL {
            YES
        }

        let superclass = class!(NSObject);
        let Some(mut decl) = ClassDecl::new("ChoroNotificationDelegate", superclass) else {
            return false;
        };
        decl.add_method(
            sel!(userNotificationCenter:didActivateNotification:),
            did_activate_notification as extern "C" fn(&Object, Sel, *mut Object, *mut Object),
        );
        decl.add_method(
            sel!(userNotificationCenter:shouldPresentNotification:),
            should_present_notification
                as extern "C" fn(&Object, Sel, *mut Object, *mut Object) -> BOOL,
        );
        let delegate_class = decl.register();
        let delegate: *mut Object = msg_send![delegate_class, new];
        if delegate.is_null() {
            return false;
        }

        let center: *mut Object = msg_send![
            class!(NSUserNotificationCenter),
            defaultUserNotificationCenter
        ];
        if center.is_null() {
            let _: () = msg_send![delegate, release];
            return false;
        }

        DELEGATE = delegate;
        let _: () = msg_send![center, setDelegate:delegate];
        true
    })
}

#[cfg(target_os = "macos")]
pub fn set_dock_badge(label: Option<&str>) {
    unsafe {
        use objc::runtime::Object;
        use objc::{class, msg_send, sel, sel_impl};

        let app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
        let dock_tile: *mut Object = msg_send![app, dockTile];
        let label = label.filter(|value| !value.trim().is_empty());
        let badge: *mut Object = label.map(ns_string).unwrap_or(std::ptr::null_mut());
        let _: () = msg_send![dock_tile, setBadgeLabel:badge];
        let _: () = msg_send![dock_tile, display];
    }
}

#[cfg(not(target_os = "macos"))]
pub fn set_dock_badge(_label: Option<&str>) {}

#[cfg(target_os = "macos")]
pub fn notify_agent_waiting(agent_title: &str, project_name: &str) {
    if !install_notification_delegate() {
        play_generated_sound();
        return;
    }

    unsafe {
        use objc::runtime::Object;
        use objc::{class, msg_send, sel, sel_impl};

        let title = ns_string("Agent needs attention");
        let body = ns_string(&format!("{agent_title} is waiting in {project_name}"));
        let sound = ns_string("Glass");

        let notification: *mut Object = msg_send![class!(NSUserNotification), alloc];
        let notification: *mut Object = msg_send![notification, init];
        if notification.is_null() {
            play_generated_sound();
            return;
        }
        let _: () = msg_send![notification, setTitle:title];
        let _: () = msg_send![notification, setInformativeText:body];
        let _: () = msg_send![notification, setSoundName:sound];

        let center: *mut Object = msg_send![
            class!(NSUserNotificationCenter),
            defaultUserNotificationCenter
        ];
        if center.is_null() {
            play_generated_sound();
        } else {
            let _: () = msg_send![center, deliverNotification:notification];
        }
        let _: () = msg_send![notification, release];
    }
}

#[cfg(not(target_os = "macos"))]
pub fn notify_agent_waiting(_agent_title: &str, _project_name: &str) {
    play_generated_sound();
}
