# Choro modifications

- Upstream: <https://github.com/SSheldon/rust-block>
- Baseline: version 0.1.6, commit `47178790cfc9d4a8b092051d8b413b78bd31254a`
- Upstream license: MIT (`LICENSE-MIT`)

Choro changed the opaque Objective-C class marker so the crate continues to
compile on newer Rust versions while preserving the public API and runtime
behavior. Packaging-only Cargo metadata was also normalized by Cargo.
