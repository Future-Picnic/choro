# Choro licensing guide

## What Apache-2.0 covers

The root Apache License 2.0 covers source and documentation authored for Choro,
unless a file or directory states different terms. The neutral copyright label
“Choro contributors” is provisional and does not itself identify or transfer
ownership. Keep evidence of authorship. If a company later becomes the owner,
use a written intellectual-property assignment; changing the name in `NOTICE`
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

## Provider terms are a separate release gate

Open-sourcing Choro under Apache-2.0 does not grant permission to use a model
provider's service or credentials. The current Claude bridge invokes the Agent
SDK with the user's locally installed Claude Code executable. Anthropic's current
legal guidance nevertheless directs third-party products using the Agent SDK to
API-key authentication through Claude Console or a supported cloud provider and
prohibits routing Free, Pro, or Max plan credentials on users' behalf.

Do not publicly distribute or sell the current subscription-authenticated Claude
integration without either changing it to an approved authentication method or
obtaining written approval from Anthropic. This is independent of the Apache-2.0
choice for Choro's source code.

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
  --editor-node-modules crates/ide-app/web/doc-editor/node_modules
```

Review `target/license-audit/third-party/INDEX.md` for missing license metadata.
An entry with no copied license file is a review item, not evidence that the
package is unlicensed.

The document editor currently brings in MPL-2.0 packages from BlockNote and its
resolved editor graph. MPL-2.0 is file-level copyleft: keep the covered package
files and modifications to those files under MPL-2.0, preserve their notices,
and make the corresponding MPL-covered source available when distributing the
application. It does not relicense Choro's separate Rust or TypeScript files.

## Hosted Penpot and browser distribution

Choro Design is a self-hosted Penpot 2.16.2 service. Penpot is MPL-2.0. Merely
making server-side functionality available over a network is not distribution
under the MPL, but the HTML, CSS, and JavaScript delivered to a user's browser
or embedded web view are distributed copies. Minified JavaScript is Executable
Form, not Source Code Form.

For every public Choro Design release:

- retain Penpot's copyright and MPL notices in the source;
- keep modifications to MPL-covered files and new files marked MPL-2.0 under
  MPL-2.0;
- publish the preferred source form for the exact frontend delivered, including
  Choro modifications, or supply it promptly and without charge through the
  source-request process at <https://choro.dev/open-source.html>;
- identify the exact upstream tag or commit and retain the integration patch
  and added source files in the public `choro-penpot` repository;
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
