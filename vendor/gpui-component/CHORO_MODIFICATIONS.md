# Choro modifications

- Upstream: <https://github.com/longbridge/gpui-component>
- Baseline: version/tag 0.5.1, commit `0f0ab35233212f8f3277028995caf0c41e13ee6c`
- Crates.io archive SHA-256: `d021d46b4088d3d93a57ccdf443da85695a77272108caca2f6fe5369f584966a`
- Upstream license: Apache-2.0 (`LICENSE-APACHE`)

Choro carries local compatibility, component behavior, design-system, editor,
input, table, theme, and integration changes. Relative to the published 0.5.1
crate, 92 Rust source files are modified. Every modified source file carries a
prominent notice pointing to this record.

The local `TextView` also supports transient, full-Unicode case-insensitive match
highlights. Find-in-conversation uses this render-only layer so Markdown is not
reparsed and off-screen virtualized messages do not create highlight elements.

To review the patch against a local Cargo registry checkout:

```sh
diff -ru path/to/gpui-component-0.5.1 vendor/gpui-component
```
