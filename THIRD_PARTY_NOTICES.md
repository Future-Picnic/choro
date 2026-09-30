# Third-party notices

This document identifies the principal third-party components distributed in
Choro source or desktop builds. It does not replace the license text shipped by
each component. Packaged applications include a generated, version-specific
dependency inventory and the license files found in the resolved Rust and Node
packages under `Contents/Resources/licenses/third-party`.

## Vendored source

| Component | Baseline | License | Upstream |
| --- | --- | --- | --- |
| `block` | 0.1.6 / commit `47178790cfc9d4a8b092051d8b413b78bd31254a` | MIT | <https://github.com/SSheldon/rust-block> |
| `gpui-component` | 0.5.1 / commit `0f0ab35233212f8f3277028995caf0c41e13ee6c` | Apache-2.0 | <https://github.com/longbridge/gpui-component> |
| `gpui-terminal` | 0.1.0 / commit `45c63e57181d27c260124a81c7e4b68a6b6e57b0` | MIT OR Apache-2.0 | <https://github.com/zortax/gpui-terminal> |
| `velotype` | 0.6.0 / commit `7802fb1d02fae12cf95f5308a3568a27cdcefa32` | Apache-2.0 | <https://github.com/manyougz/velotype> |

These directories contain Choro-specific modifications. Each directory carries
its upstream license and a `CHORO_MODIFICATIONS.md` record. Modified Apache-2.0
source files also carry a prominent Choro modification notice.

## Principal Rust components

Choro uses GPUI 0.2.2 and `gpui-component-assets` under Apache-2.0,
`agent-client-protocol` under Apache-2.0, and database/network libraries under
their respective permissive licenses. The resolved graph also contains
file-level MPL-2.0 components. MPL-covered files remain available from their
listed upstream package sources and retain MPL-2.0; they do not change the
license of Choro's separate first-party files.

The build-time license collector records every resolved Cargo package, its
version, declared SPDX expression, repository, and copied license files in the
desktop application.

Choro uses the official `libgit2-sys` package with its bundled libgit2 library.
The bindings use MIT OR Apache-2.0; libgit2 uses GPL-2.0 with a linking exception.
The collector includes libgit2's `COPYING` from the resolved Cargo package.

## Local voice models

Project Talk and dictation download their optional model payload only after the user opts
in. Moonshine Small Streaming English and the Moonshine runtime are provided by
Moonshine AI; the English-language speech models and Moonshine code are MIT
licensed. Smart Turn v3.2 is provided by Daily under the BSD 2-Clause License.
The exact license texts are included in the packaged license materials. Choro
pins and verifies the SHA-256 digest of every downloaded model artifact.

## Embedded BlockNote editor

Choro embeds the unmodified BlockNote Core, React, and Mantine packages for its
document editor. BlockNote Core 0.51.4 is distributed under MPL-2.0; its source
is available from <https://github.com/TypeCellOS/BlockNote/tree/v0.51.4>.
MPL-covered package files remain under MPL-2.0 and are kept separate from
Choro's first-party Rust and TypeScript source. Choro does not include the
commercial `@blocknote/xl-*` packages.

## Claude Agent SDK

`@anthropic-ai/claude-agent-sdk` is proprietary software from Anthropic PBC. It
is not licensed under Apache-2.0 and is not part of Choro's first-party license
grant. Use is subject to Anthropic's applicable legal agreements:

<https://code.claude.com/docs/en/legal-and-compliance>

Choro invokes the SDK on the user's computer with the user's own installed
Claude Code executable. Users authenticate through Claude Code itself. Choro
does not offer a Claude.ai login, collect or store Claude credentials or session
tokens, proxy Claude requests through a Choro server, or pay for users' Claude
usage. Anthropic's guidance permits end users to sign in to an unmodified Claude
Code binary with their own subscription, while recommending API-key
authentication for developers building products with the Agent SDK. The SDK's
Commercial Terms also apply to products offered to end users. Review those
terms and the bundled SDK package for the release; this notice does not claim
Anthropic's endorsement or approval of Choro.

## Fonts and icon libraries

| Asset | License | Source |
| --- | --- | --- |
| Inter | SIL Open Font License 1.1 | <https://github.com/rsms/inter> |
| Schibsted Grotesk | SIL Open Font License 1.1 | <https://github.com/schibsted/schibsted-grotesk> |
| Devicon database subset | MIT | <https://github.com/devicons/devicon> |
| Lucide icons/font | MIT and ISC as declared by the package | <https://github.com/lucide-icons/lucide> |

The corresponding font license files and the Devicon subset provenance record
are stored beside the assets and copied into packaged applications.

## README technology marks

The unmodified Rust logo files in `assets/readme/` come from the
[Rust artwork repository](https://github.com/rust-lang/rust-artwork/tree/main/logo).
The Rust Foundation distributes them under
[CC BY 4.0](https://creativecommons.org/licenses/by/4.0/); Rust's
[trademark policy](https://rust-lang.org/policies/media-guide) also applies.
The Turso logomarks in the same directory are unmodified files from
[Turso's brand kit](https://turso.tech/brand). They are used only to identify
the local embedded database technology. GPUI is identified by name without
using Zed's logo. None of these marks is included in Choro's Apache-2.0
license grant, and no endorsement is implied.

## Product and provider marks

The Asana, Anthropic/Claude, Atlassian/Jira, ClickUp, Figma, Linear, OpenAI,
and other product names and logos are used only to identify compatible
integrations or upstream software.
They are excluded from Choro's Apache-2.0 license grant and remain subject to
their owners' copyright, trademark, and brand-usage rules. No endorsement is
claimed.

See `crates/ide-app/assets/ASSET_PROVENANCE.md` for the repository asset map.
