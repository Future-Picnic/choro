// Copyright 2020-2024 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

#[cfg(target_os = "macos")]
use objc2::DefinedClass;
use objc2::{define_class, msg_send, rc::Retained, MainThreadOnly};
#[cfg(target_os = "macos")]
use objc2_app_kit::{
  NSApplication, NSEvent, NSEventModifierFlags, NSView, NSWindow, NSWindowButton,
};
use objc2_foundation::MainThreadMarker;
#[cfg(target_os = "macos")]
use objc2_foundation::{NSArray, NSRect};
#[cfg(target_os = "ios")]
use objc2_ui_kit::UIView as NSView;

pub struct WryWebViewParentIvars {
  #[cfg(target_os = "macos")]
  traffic_light_inset: std::cell::Cell<Option<(f64, f64)>>,
}

#[cfg(target_os = "macos")]
fn should_offer_key_event_to_menu(flags: NSEventModifierFlags) -> bool {
  flags.intersects(NSEventModifierFlags::Command | NSEventModifierFlags::Control)
}

define_class!(
  #[unsafe(super(NSView))]
  #[name = "WryWebViewParent"]
  #[ivars = WryWebViewParentIvars]
  pub struct WryWebViewParent;

  /// Overridden NSView methods.
  impl WryWebViewParent {
    #[cfg(target_os = "macos")]
    #[unsafe(method(keyDown:))]
    fn key_down(&self, event: &NSEvent) {
      let flags = unsafe { event.modifierFlags() };

      // Only offer real menu shortcuts to AppKit. Wry 0.52.1 sent every
      // keyDown here and never forwarded unhandled events, swallowing ordinary
      // text, number, symbol, and arrow keys used by iframe-heavy web apps.
      // Option is intentionally excluded because it participates in macOS
      // special-character and dead-key input.
      if should_offer_key_event_to_menu(flags) {
        let mtm = MainThreadMarker::new().unwrap();
        let app = NSApplication::sharedApplication(mtm);
        unsafe {
          if let Some(menu) = app.mainMenu() {
            if menu.performKeyEquivalent(event) {
              return;
            }
          }
        }
      }

      // Backport of https://github.com/tauri-apps/wry/pull/1711. Interpret
      // everything the menu did not handle so WebKit's responder chain can
      // deliver ordinary keyboard input without reintroducing AppKit's beep.
      unsafe {
        self.interpretKeyEvents(&NSArray::from_slice(&[event]));
      }
    }

    #[cfg(target_os = "macos")]
    #[unsafe(method(drawRect:))]
    fn draw(&self, _dirty_rect: NSRect) {
      if let Some((x, y)) = self.ivars().traffic_light_inset.get() {
        unsafe { inset_traffic_lights(&self.window().unwrap(), x, y) };
      }
    }
  }
);

impl WryWebViewParent {
  #[allow(dead_code)]
  pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
    let delegate = WryWebViewParent::alloc(mtm).set_ivars(WryWebViewParentIvars {
      #[cfg(target_os = "macos")]
      traffic_light_inset: Default::default(),
    });
    unsafe { msg_send![super(delegate), init] }
  }

  #[cfg(target_os = "macos")]
  pub fn set_traffic_light_inset(&self, ns_window: &NSWindow, position: dpi::Position) {
    let scale_factor = NSWindow::backingScaleFactor(ns_window);
    let position = position.to_logical(scale_factor);
    self
      .ivars()
      .traffic_light_inset
      .replace(Some((position.x, position.y)));

    unsafe {
      inset_traffic_lights(ns_window, position.x, position.y);
    }
  }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
  use super::*;

  #[test]
  fn ordinary_and_option_modified_input_bypass_menu_dispatch() {
    assert!(!should_offer_key_event_to_menu(
      NSEventModifierFlags::empty()
    ));
    assert!(!should_offer_key_event_to_menu(
      NSEventModifierFlags::Option
    ));
    assert!(should_offer_key_event_to_menu(
      NSEventModifierFlags::Command
    ));
    assert!(should_offer_key_event_to_menu(
      NSEventModifierFlags::Control
    ));
  }
}

#[cfg(target_os = "macos")]
pub unsafe fn inset_traffic_lights(window: &NSWindow, x: f64, y: f64) {
  let Some(close) = window.standardWindowButton(NSWindowButton::CloseButton) else {
    #[cfg(feature = "tracing")]
    tracing::warn!("skipping inset_traffic_lights, close button not found");
    return;
  };
  let Some(miniaturize) = window.standardWindowButton(NSWindowButton::MiniaturizeButton) else {
    #[cfg(feature = "tracing")]
    tracing::warn!("skipping inset_traffic_lights, miniaturize button not found");
    return;
  };
  let zoom = window.standardWindowButton(NSWindowButton::ZoomButton);

  let title_bar_container_view = close.superview().unwrap().superview().unwrap();

  let close_rect = NSView::frame(&close);
  let title_bar_frame_height = close_rect.size.height + y;
  let mut title_bar_rect = NSView::frame(&title_bar_container_view);
  title_bar_rect.size.height = title_bar_frame_height;
  title_bar_rect.origin.y = window.frame().size.height - title_bar_frame_height;
  title_bar_container_view.setFrame(title_bar_rect);

  let space_between = NSView::frame(&miniaturize).origin.x - close_rect.origin.x;

  let mut window_buttons = vec![close, miniaturize];
  if let Some(zoom) = zoom {
    window_buttons.push(zoom);
  }

  for (i, button) in window_buttons.into_iter().enumerate() {
    let mut rect = NSView::frame(&button);
    rect.origin.x = x + (i as f64 * space_between);
    button.setFrameOrigin(rect.origin);
  }
}
