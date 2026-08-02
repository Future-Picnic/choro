# Choro scripts

## App builders

These are the only three app modes:

- `bundle.sh` builds the normal `Choro.app` release.
- `bundle-new-user.sh` builds `Choro New User.app` with a unique empty data
  directory and unique installation and Design keychain services. It exercises
  the real first-run onboarding and managed Design provisioning flow.
- `bundle-demo.sh` builds `Choro Demo.app`. Every launch resets its isolated
  data, uses fresh Demo-only keychain namespaces, and recreates the fictional
  demo workspace from tracked fixtures.

Each builder creates its app under `target/release/bundle/`. By default it also
installs its distinctly named app in `/Applications`; set the matching
`CHORO_*_INSTALL_TO_APPLICATIONS=0` variable to build without installing.

## Support utilities

The other files are not additional app modes:

- `verify.sh` runs the local verification baseline.
- `gen_themes.py` regenerates the Choro theme asset.
- `collect-third-party-licenses.mjs` assembles release license material.
- `list-agent-runtime-skills.mjs` is the one runtime helper included in apps.

Penpot deployment and source packaging live in the sibling public
`choro-penpot` project. The feedback receiver lives in the private
`choro-relay` project.
