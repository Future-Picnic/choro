# Vendored dependencies

Choro vendors a small set of crates because its GPUI/editor stack currently needs local compatibility or integration changes. Keep each dependency's own license files intact and review upstream changes before replacing a vendored directory.

| Directory | Packaged version | Upstream | Local reason |
| --- | ---: | --- | --- |
| <code>block</code> | 0.1.6 | <https://github.com/SSheldon/rust-block> | Compatibility patch for newer Rust's handling of the opaque Objective-C class marker; see the root <code>[patch.crates-io]</code> comment. |
| <code>gpui-component</code> | 0.5.1 | <https://github.com/longbridge/gpui-component> | Local GPUI component integration and feature alignment. |
| <code>gpui-terminal</code> | 0.1.0 | <https://github.com/zortax/gpui-terminal> | Local terminal integration against Choro's GPUI dependency graph. |
| <code>velotype</code> | 0.6.0 | <https://github.com/manyougz/velotype> | Embedded editor integration, native HTML support, syntax highlighting, and local test/harness fixes. |

Each directory now includes a <code>CHORO_MODIFICATIONS.md</code> record with its
verified upstream baseline, license, and a summary of Choro-specific changes.
Modified Apache-2.0 source files carry a prominent modification notice.

The GPUI component, terminal, and Velotype directories retain their upstream
license files. The <code>block</code> directory includes the MIT text corresponding
to the license declared by its upstream package metadata.
