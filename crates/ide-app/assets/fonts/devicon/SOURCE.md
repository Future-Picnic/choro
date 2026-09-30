# Devicon database font subset

- Upstream: https://github.com/devicons/devicon
- Version: v2.17.0
- Source font: fonts/devicon.ttf
- License: MIT
- Subset SHA-256: 22e107cafaddc3e4006dc33063c53ed05917cd007ae7bd9c3fa466010ec65879

The bundled font is a deterministic subset containing only the six glyphs
used by Choro's database UI:

- MariaDB: U+EAD9
- MongoDB: U+EAF5
- MySQL: U+EAFD
- PostgreSQL: U+EB79
- SQLite: U+EC1E
- Supabase: U+EC2E

The subset also contains a blank ASCII m glyph. GPUI 0.2.2 intentionally
rejects fonts without that measurement glyph on macOS.

It is stored as Base64 text so the vendored subset remains diff-auditable.
Choro decodes the 6.1 KiB TTF once while installing UI fonts.
