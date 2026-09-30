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

By default, all app modes use an ad-hoc macOS signature for local development,
so contributors can build without an Apple Developer identity. Release or CI
builds can set `CHORO_CODESIGN_IDENTITY` to a complete signing identity name or
hash in their private environment. The repository does not contain or
automatically select an organization or contributor identity.

## Support utilities

The other files are not additional app modes:

- `rebuild-and-relaunch.sh` opens an external macOS Terminal, asks for
  confirmation, rebuilds the normal app, quits the installed Choro only after
  a successful build, replaces `/Applications/Choro.app`, and reopens it. Run
  it from any terminal with `./scripts/rebuild-and-relaunch.sh`.
- `verify.sh` runs the local verification baseline.
- `gen_themes.py` regenerates the Choro theme asset.
- `collect-third-party-licenses.mjs` assembles release license material.
- `list-agent-runtime-skills.mjs` is the one runtime helper included in apps.

The feedback receiver lives in the private `choro-relay` project.

## iPhone releases

Run `./scripts/release-ios.sh` to build Choro Remote locally and upload it to
App Store Connect for TestFlight. It uses the private sibling checkout at
`../choro-relay/apps/choro-remote`; set `CHORO_MOBILE_ROOT` if it lives elsewhere.
Xcode uses the existing FuturePicnic Apple signing setup and manages the uploaded
build number. The current mobile working copy is copied into an isolated release
directory; this command does not commit, push, or publish a public App Store release.

```bash
./scripts/release-ios.sh --dry-run
./scripts/release-ios.sh                 # Build, verify, upload for TestFlight
./scripts/release-ios.sh --version 0.1.1 # Choose a new marketing version
./scripts/release-ios.sh --build-only    # Produce a local IPA without uploading
```

See the mobile checkout's `RELEASE.md` for prerequisites, retained artifacts,
resuming an upload, and Apple's processing and export-compliance steps.
