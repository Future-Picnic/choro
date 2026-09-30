// Copyright 2020-2024 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use std::ffi::CStr;

use http::Request;
use objc2::{
  define_class, msg_send,
  rc::Retained,
  runtime::{NSObject, ProtocolObject},
  DeclaredClass, MainThreadOnly,
};
use objc2_foundation::{MainThreadMarker, NSObjectProtocol, NSString};
use objc2_web_kit::{WKScriptMessage, WKScriptMessageHandler, WKUserContentController};

pub const IPC_MESSAGE_HANDLER_NAME: &str = "ipc";

pub struct WryWebViewDelegateIvars {
  pub controller: Retained<WKUserContentController>,
  pub ipc_handler: Box<dyn Fn(Request<String>)>,
}

define_class!(
  #[unsafe(super(NSObject))]
  #[name = "WryWebViewDelegate"]
  #[thread_kind = MainThreadOnly]
  #[ivars = WryWebViewDelegateIvars]
  pub struct WryWebViewDelegate;

  unsafe impl NSObjectProtocol for WryWebViewDelegate {}

  unsafe impl WKScriptMessageHandler for WryWebViewDelegate {
    // Function for ipc handler
    #[unsafe(method(userContentController:didReceiveScriptMessage:))]
    fn did_receive(
      this: &WryWebViewDelegate,
      _controller: &WKUserContentController,
      msg: &WKScriptMessage,
    ) {
      // Safety: objc runtime calls are unsafe
      unsafe {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!(parent: None, "wry::ipc::handle").entered();

        let ipc_handler = &this.ivars().ipc_handler;
        let body = msg.body();
        if let Ok(body) = body.downcast::<NSString>() {
          let js_utf8 = body.UTF8String();

          let frame_info = msg.frameInfo();
          let request = frame_info.request();
          let Some(url) = request.URL() else {
            #[cfg(feature = "tracing")]
            tracing::warn!("WebView IPC call had no source URL.");
            return;
          };
          let Some(absolute_url) = url.absoluteString() else {
            #[cfg(feature = "tracing")]
            tracing::warn!("WebView IPC call had no absolute source URL.");
            return;
          };
          let url_utf8 = absolute_url.UTF8String();

          if js_utf8.is_null() || url_utf8.is_null() {
            #[cfg(feature = "tracing")]
            tracing::warn!("WebView IPC call contained a non-UTF-8 URL or body.");
            return;
          }

          if let (Ok(url), Ok(js)) = (
            CStr::from_ptr(url_utf8).to_str(),
            CStr::from_ptr(js_utf8).to_str(),
          ) {
            // A page can acquire an invalid HTTP URI after WebKit handles an
            // external file drop (for example a local path containing spaces).
            // Never let that untrusted page URL unwind through Objective-C.
            if let Some(request) = ipc_request(url, js.to_string()) {
              ipc_handler(request);
            } else {
              #[cfg(feature = "tracing")]
              tracing::warn!("WebView received IPC from an invalid source URL: {}", url);
            }
            return;
          }
        }

        #[cfg(feature = "tracing")]
        tracing::warn!("WebView received invalid IPC call.");
      }
    }
  }
);

fn ipc_request(url: &str, body: String) -> Option<Request<String>> {
  Request::builder().uri(url).body(body).ok()
}

impl WryWebViewDelegate {
  pub fn new(
    controller: Retained<WKUserContentController>,
    ipc_handler: Box<dyn Fn(Request<String>)>,
    mtm: MainThreadMarker,
  ) -> Retained<Self> {
    let delegate = mtm
      .alloc::<WryWebViewDelegate>()
      .set_ivars(WryWebViewDelegateIvars {
        ipc_handler,
        controller,
      });

    let delegate: Retained<Self> = unsafe { msg_send![super(delegate), init] };

    let proto_delegate = ProtocolObject::from_ref(&*delegate);
    unsafe {
      // this will increate the retain count of the delegate
      delegate.ivars().controller.addScriptMessageHandler_name(
        proto_delegate,
        &NSString::from_str(IPC_MESSAGE_HANDLER_NAME),
      );
    }

    delegate
  }
}

#[cfg(test)]
mod tests {
  use super::ipc_request;

  #[test]
  fn invalid_external_file_url_does_not_build_an_ipc_request() {
    assert!(ipc_request("file:///Users/choro/Outside file.pdf", "{}".into()).is_none());
  }

  #[test]
  fn valid_webview_url_still_builds_an_ipc_request() {
    let request = ipc_request("choro-editor://localhost/index.html", "{}".into())
      .expect("valid custom-protocol URL");

    assert_eq!(request.uri(), "choro-editor://localhost/index.html");
    assert_eq!(request.body(), "{}");
  }
}
