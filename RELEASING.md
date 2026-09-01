# Releasing Choro for macOS

Choro releases are built locally from a clean, up-to-date `dev` branch and are
published to the private `Future-Picnic/choro` GitHub repository. The release
command never deletes or overwrites an existing tag, release asset, bundle, or
staging directory.

## One-time setup

1. Connect a GitHub CLI account with administration access to the repository.
2. Keep `Developer ID Application: Liran Gabai (NQTUZ98HJZ)` in the login Keychain.
3. Store Apple notarization credentials as the `choro-notary` notarytool
   Keychain profile.
4. Keep the Sparkle Ed25519 private key in Keychain under account `choro`. Its
   public half is tracked in `release/sparkle-public-key.txt`.

The private Sparkle key is never stored in this repository. Back it up through
the existing secure credential process before using it for production updates.

## Validate the next release

```sh
./scripts/release-macos.sh --dry-run
```

## Build and publish

```sh
./scripts/release-macos.sh --notes-file /absolute/path/to/release-notes.txt
```

Without `--notes-file`, the command derives notes from first-parent commits
since the previous version tag. It computes the next sequential `0.N` version,
builds and validates locally, then requires typing the exact tag before any
Apple, Git, or GitHub mutation.

If a confirmed release fails, inspect its preserved
`target/release/releases/v0.N/release-state.json` and resume without replacing
existing assets:

```sh
./scripts/release-macos.sh --resume v0.N
```

An interrupted build, package, or download may leave a hidden `.partial.*`
file beside the release output. Resume never consumes, removes, or overwrites
those incomplete files; they remain available for inspection.

Version `0.89` is the manually installed updater bootstrap. The first complete
in-app update validation is performed by installing `0.89` and publishing a
controlled `0.90` release through the same command.
