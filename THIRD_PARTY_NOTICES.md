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
| `libgit2-sys` / libgit2 | 0.18.5+1.9.4 / 1.9.4 | MIT OR Apache-2.0 (bindings); GPL-2.0 with linking exception (libgit2; see `COPYING`) | <https://github.com/rust-lang/git2-rs>, <https://github.com/libgit2/libgit2> |
| `gpui-component` | 0.5.1 / commit `0f0ab35233212f8f3277028995caf0c41e13ee6c` | Apache-2.0 | <https://github.com/longbridge/gpui-component> |
| `gpui-terminal` | 0.1.0 / commit `45c63e57181d27c260124a81c7e4b68a6b6e57b0` | MIT OR Apache-2.0 | <https://github.com/zortax/gpui-terminal> |
| `velotype` | 0.6.0 / commit `7802fb1d02fae12cf95f5308a3568a27cdcefa32` | Apache-2.0 | <https://github.com/manyougz/velotype> |

These directories contain Choro-specific modifications. Each directory carries
its upstream license and a `CHORO_MODIFICATIONS.md` record (or `CHORO_PATCH.md`
for `libgit2-sys`). Modified Apache-2.0
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

## Hosted Choro Design service

Choro Design is a Choro-operated, self-hosted deployment based on Penpot
2.16.2. Penpot is licensed under the Mozilla Public License 2.0; the upstream
source for the deployed baseline is available at:

<https://github.com/penpot/penpot/tree/2.16.2>

The hosted Penpot application is not bundled inside the Choro desktop DMG.
However, its frontend code is delivered to users' web views and browsers.
Choro modifications to MPL-covered Penpot files, and new integration files
marked MPL-2.0, remain under MPL-2.0. Their corresponding source must be made
available to recipients while the executable form is offered. The public
source-availability notice is published at
<https://choro.dev/open-source.html>. The Choro change source and the script
that assembles it with the exact upstream source are maintained in the public
`choro-penpot` repository.

Separate Choro-authored deployment, provisioning, styling, and integration
files that contain no MPL-covered code remain under their stated licences.
Penpot's file-level copyleft does not change the licence of those separate
files or of Choro's desktop source.

## Claude Agent SDK

`@anthropic-ai/claude-agent-sdk` is proprietary software from Anthropic PBC. It
is not licensed under Apache-2.0 and is not part of Choro's first-party license
grant. Use is subject to Anthropic's applicable legal agreements:

<https://code.claude.com/docs/en/legal-and-compliance>

Choro requires users to install and authenticate Claude Code for themselves and
passes the path of that local executable to the SDK. Choro does not implement a
Claude.ai login flow, copy OAuth credentials, or share a subscription between
users. That technical design alone is not permission to route subscription
credentials through a third-party product. Anthropic's current guidance says
developers building products or services with the Agent SDK should use API-key
authentication through Claude Console or a supported cloud provider, and that
third-party developers may not route Free, Pro, or Max credentials on behalf of
users. Keep this integration private until it uses an approved authentication
method or Anthropic confirms the intended distribution model in writing.

## Fonts and icon libraries

| Asset | License | Source |
| --- | --- | --- |
| Inter | SIL Open Font License 1.1 | <https://github.com/rsms/inter> |
| Schibsted Grotesk | SIL Open Font License 1.1 | <https://github.com/schibsted/schibsted-grotesk> |
| Devicon database subset | MIT | <https://github.com/devicons/devicon> |
| Lucide icons/font | MIT and ISC as declared by the package | <https://github.com/lucide-icons/lucide> |

The corresponding font license files and the Devicon subset provenance record
are stored beside the assets and copied into packaged applications.

## Product and provider marks

The Asana, Anthropic/Claude, Atlassian/Jira, ClickUp, Figma, Linear, OpenAI,
Penpot, and other product names and logos are used only to identify compatible
integrations or upstream software.
They are excluded from Choro's Apache-2.0 license grant and remain subject to
their owners' copyright, trademark, and brand-usage rules. No endorsement is
claimed.

See `crates/ide-app/assets/ASSET_PROVENANCE.md` for the repository asset map.
