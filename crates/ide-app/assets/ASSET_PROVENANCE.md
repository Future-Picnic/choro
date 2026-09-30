# Asset provenance

This file records the licensing boundary for assets stored in the desktop app.

## Choro project assets

The application icon concepts, Choro logo/wordmark artwork, product screenshots,
empty-state illustrations, and custom neutral UI glyphs were created for the
Choro project and are covered by the root Apache-2.0 license unless noted below.

The `add-row.svg`, `file.svg`, `url.svg`, and `operations-icon.svg` files were
redrawn for Choro in July 2026 to remove previously unrecorded external SVG
provenance.

## Licensed third-party assets

| Path | Source | Terms |
| --- | --- | --- |
| `fonts/inter/` | Inter project | SIL OFL 1.1; see `fonts/inter/LICENSE.txt` |
| `fonts/schibsted/` | Schibsted Grotesk project | SIL OFL 1.1; see `fonts/schibsted/OFL.txt` |
| `fonts/devicon/` | Devicon v2.17.0 subset | MIT; see `fonts/devicon/LICENSE` and `SOURCE.md` |
| Lucide-derived glyphs and embedded font | Lucide | MIT/ISC; provided by the `lucide-icons` crate |

## Identification and trademark assets

Files under `brand/`, `agent-icons/claude.svg`, `agent-icons/openai.svg`, and
`icons/figma.svg` identify external products. They are excluded from Choro's
Apache-2.0 grant. All rights in those names and marks remain with their
respective owners. Use must remain referential, must not imply endorsement, and
must follow the applicable brand guidelines.

## Adding assets

Every new third-party asset must record its source URL, version or retrieval
date, license, required attribution, and any modifications in this file or a
`SOURCE.md` beside the asset. Do not add an asset whose redistribution terms
cannot be established.
