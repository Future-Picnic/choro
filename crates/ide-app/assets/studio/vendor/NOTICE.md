# VvvebJs editor integration for Choro Studio

Source: https://github.com/givanz/VvvebJs/tree/1acbab7ebfe3e7b004f1f18c039d26550fc04bd8
Pinned upstream revision: `1acbab7ebfe3e7b004f1f18c039d26550fc04bd8` (2.0.9).
Copyright 2017 Ziadin Givan. Apache-2.0; see LICENSE.

`upstream/` contains unmodified upstream builder, input renderers, common and
HTML component properties, undo, autocomplete, editor stylesheet, Bootstrap,
Popper, and Coloris files. The right-panel and inline-toolbar HTML fragments
are extracted verbatim from upstream `editor.html`. Source paths and SHA-256
hashes are recorded in `upstream-manifest.json`.

`templates.js` contains upstream's templates precompiled using upstream's own
John Resig template compiler (MIT). This avoids allowing runtime `unsafe-eval`.
`fonts.css` embeds the original icon fonts as local data URLs. Regenerate these
artifacts with `python3 vendor/import.py /path/to/pinned/VvvebJs`.

Choro's integration is separate, in `../editor.js`, `../editor.html`, and the
native Studio host. It connects Vvveb controls to the isolated screen document,
local asset imports, design tokens, source-preserving saves and revisioned
transactions. Layout and theme overrides fit the upstream controls inside
Choro. The full builder contains other modules, but Choro does not initialize
its page manager, galleries, file services, full application shell, or AI.

The old top-level `builder.js` and `undo.js` are historical subset files and
are not loaded by Studio. The active upstream files are under `upstream/`.

## Third-party notices

- Bootstrap 5.3.3: MIT; `bootstrap-LICENSE.txt`.
- Popper 2.9.2: MIT; `popper-LICENSE.txt`.
- Coloris: MIT; `coloris-LICENSE.txt`.
- Line Awesome by Icons8: MIT option; `line-awesome-LICENSE.txt` and
  `line-awesome-MIT.txt`. https://icons8.com/line-awesome
- Ionicons: MIT; `ionicons-LICENSE.txt`.
- John Resig's template compiler: MIT; `template-LICENSE.txt`.

Original copyright/license headers are retained in the vendored files.
