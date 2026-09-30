# Shared signing-identity selection for every Choro app bundle.

resolve_choro_codesign_identity() {
  if [[ -n "${CHORO_CODESIGN_IDENTITY:-}" ]]; then
    print -r -- "$CHORO_CODESIGN_IDENTITY"
  else
    # Ad-hoc signing keeps local bundles runnable without selecting a personal
    # Apple Development identity from the developer's keychain.
    print -r -- "-"
  fi
}
