# Choro project conventions

## UI controls

- New or modified feature UI must construct buttons through the shared builders in `crates/ide-app/src/ui/style.rs` (or a canonical builder in `ui/design`). Do not instantiate `gpui_component::Button` directly or apply raw size/color variants in feature modules.
- Modal primary actions use `primary_button_compact`; modal neutral and Cancel actions use `dialog_neutral_button`; destructive modal actions use `danger_button_compact`.
- Toolbar icon actions use `header_icon_button` or the purpose-specific helpers such as `refresh_icon_button`. If the design system lacks a needed treatment, add a reusable helper before using it in a feature.

## Verification

- After Rust changes, test the affected crate with its default features. For `ide-app`, also run `cargo test --locked -p ide-app --features ui-layout-tests` when changing UI tests. A production build alone does not compile `#[cfg(test)]` code.
- Gate GPUI test fixtures and tests that require `TestAppContext` behind `ui-layout-tests`; keep ordinary unit tests available with default features.
- Before reporting that a change is ready for a macOS release, run `./scripts/release-macos.sh --check`. This shares the release's Rust tests and production build without publishing. Report any failed or unrun checks.
