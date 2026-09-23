# Choro licensing guide

## What Apache-2.0 covers

The root Apache License 2.0 covers source and documentation authored for Choro,
unless a file or directory states different terms. The first-party `NOTICE`
identifies Liran Gabai, the current individual operator and sole creator.
Keep evidence of authorship. If a company later becomes the owner, use a
written intellectual-property assignment; changing the name in `NOTICE`
alone does not transfer copyright. That ownership step does not change the
dependency compatibility analysis.

Apache-2.0 permits private use, modification, redistribution, and commercial
products. Distributions must preserve the license, applicable notices, and
prominent notices on modified Apache-licensed files.

## What it does not cover

The root license does not relicense:

- files under `vendor/`, which retain their upstream licenses;
- third-party packages resolved through Cargo or npm;
- fonts and icon libraries carrying their own license files;
- the proprietary Claude Agent SDK;
- third-party names, logos, and other brand assets.

## Provider terms are separate from the source license

Open-sourcing Choro under Apache-2.0 does not grant rights to a model
provider's software or service. Choro invokes the Claude Agent SDK locally and
points it at the user's own installed Claude Code executable. The user signs in
through Claude Code's own flow; Choro does not provide a Claude.ai login, collect
or store Claude credentials or session tokens, proxy Claude requests through a
Choro server, or pay for the user's Claude usage.

Anthropic's published guidance expressly allows an end user to sign in to an
unmodified Claude Code binary with their own subscription. It also says
developers building products with the Agent SDK should use API-key
authentication, and the SDK is governed by Anthropic's Commercial Terms even
when used in products for end users. Those statements do not establish a
categorical ban on Choro's local, user-authenticated flow, but neither does the
existence of similar products establish Anthropic's approval. Review the
applicable terms and bundled SDK binary before release; seek written guidance
from Anthropic if a definitive answer for this exact integration is needed.

## Source and binary releases

Source releases must include `LICENSE`, `NOTICE`, `THIRD_PARTY_NOTICES.md`, all
license files within vendored directories, and the asset provenance record.

The macOS bundler runs `scripts/collect-third-party-licenses.mjs`. It copies the
root notices, vendored/font licenses, and a generated inventory with package
license files for the exact Cargo and Node dependencies included in that build.

Before publishing a release, run:

```sh
cargo metadata --locked --format-version 1 >/dev/null
node scripts/collect-third-party-licenses.mjs --output target/license-audit \
  --agent-node-modules crates/ide-app/assets/agent-chat/node_modules \
  --editor-node-modules crates/ide-app/web/doc-editor/node_modules \
  --canvas-node-modules crates/ide-app/web/studio-canvas/node_modules
```

Review `target/license-audit/third-party/INDEX.md` for missing license metadata.
An entry with no copied license file is a review item, not evidence that the
package is unlicensed. Some older published Rust packages provide only a Cargo
license declaration; their bundled standard license text and published
metadata are explicitly labelled as such, not as an original copyright notice.
Review `docs/release-license-audit.md` before the final build.

The document editor currently brings in MPL-2.0 packages from BlockNote and its
resolved editor graph. MPL-2.0 is file-level copyleft: keep the covered package
files and modifications to those files under MPL-2.0, preserve their notices,
and make the corresponding MPL-covered source available when distributing the
application. It does not relicense Choro's separate Rust or TypeScript files.

## Deferred Penpot project — not in the Choro launch

The separate `choro-penpot` project is not part of the Choro desktop launch and
must remain private. The launch app neither offers a Penpot connection nor
provisions a hosted design account. Do not describe Penpot as a live Choro
service in launch materials or publish its source merely as part of the
desktop release.

If Penpot is offered in a future release, Penpot 2.16.2 is MPL-2.0. Merely
making server-side functionality available over a network is not distribution
under the MPL, but the HTML, CSS, and JavaScript delivered to a user's browser
or embedded web view are distributed copies. Minified JavaScript is Executable
Form, not Source Code Form.

Before any future public hosted-Penpot release:

- retain Penpot's copyright and MPL notices in the source;
- keep modifications to MPL-covered files and new files marked MPL-2.0 under
  MPL-2.0;
- publish the preferred source form for the exact frontend delivered, including
  Choro modifications, or supply it promptly and without charge through a
  public source-request process;
- identify the exact upstream tag or commit and retain the integration patch
  and added source files in a reviewed public `choro-penpot` snapshot;
- keep the public source notice available for as long as the corresponding
  executable frontend is offered; and
- do not imply that Kaleidos operates or endorses Choro Design.

The MPL is file-level copyleft. A separate Choro provisioner, proxy
configuration, stylesheet, or script that contains no copied MPL code does not
become MPL-covered merely because it interoperates with Penpot. If code is
copied from or inserted into a Penpot source file, treat that resulting covered
file as MPL-2.0.

## Contributions

Until a separate contributor agreement or developer certificate is adopted,
contributors submit changes under the repository's Apache-2.0 terms. If Choro
later needs proprietary relicensing of community contributions, establish that
policy before accepting those contributions.
