# Choro Wry backport

This directory vendors the crates.io release of Wry 0.52.1.

Choro carries two macOS hardening changes:

- A keyboard-event fix backported from
  [tauri-apps/wry#1711](https://github.com/tauri-apps/wry/pull/1711). Wry's
  `WryWebViewParent` previously offered every key event to the application menu
  and swallowed events the menu did not handle. The backport limits menu
  handling to Command/Control shortcuts and forwards other events through
  AppKit's text input path, allowing text fields and keyboard controls in
  embedded web apps to work normally.
- The IPC delegate validates WebKit's source URL before constructing an HTTP
  request. External file drops can make WebKit report a local URL that `http`
  rejects (for example, a path containing a literal space); Wry 0.52.1 used
  `unwrap` there and aborted the whole app. Invalid IPC calls are now ignored.

The root workspace pins this copy through `[patch.crates-io]` so the fix remains
in place without adopting an unreleased Wry version.
