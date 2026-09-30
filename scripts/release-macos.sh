#!/bin/zsh
set -euo pipefail

cd "$(dirname "$0")/.."

REPOSITORY="Future-Picnic/choro"
BRANCH="dev"
SIGN_IDENTITY="${CHORO_RELEASE_SIGN_IDENTITY:-Developer ID Application: Liran Gabai (NQTUZ98HJZ)}"
NOTARY_PROFILE="${CHORO_NOTARY_PROFILE:-choro-notary}"
SPARKLE_ACCOUNT="${CHORO_SPARKLE_ACCOUNT:-choro}"
DRY_RUN=0
REQUESTED_VERSION=""
NOTES_SOURCE=""
RESUME_TAG=""
CURRENT_STAGE="preflight"
STATE_FILE=""

usage() {
  echo "Usage: ./scripts/release-macos.sh [--version 0.N] [--notes-file PATH] [--dry-run] [--resume v0.N]"
}

while (( $# > 0 )); do
  case "$1" in
    --version)
      REQUESTED_VERSION="${2:-}"
      shift 2
      ;;
    --notes-file)
      NOTES_SOURCE="${2:-}"
      shift 2
      ;;
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    --resume)
      RESUME_TAG="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ -n "$RESUME_TAG" && -n "$REQUESTED_VERSION" ]]; then
  echo "Use either --resume or --version, not both." >&2
  exit 2
fi

CURRENT_VERSION="$(tr -d '[:space:]' < VERSION)"
if [[ ! "$CURRENT_VERSION" =~ '^0\.[1-9][0-9]*$' ]]; then
  echo "VERSION must contain 0.N; found '$CURRENT_VERSION'." >&2
  exit 1
fi

if [[ -n "$RESUME_TAG" ]]; then
  if [[ ! "$RESUME_TAG" =~ '^v0\.[1-9][0-9]*$' ]]; then
    echo "--resume expects a tag such as v0.89." >&2
    exit 2
  fi
  CANDIDATE_VERSION="${RESUME_TAG#v}"
  RESUMING=1
  CURRENT_BUILD="${CURRENT_VERSION#0.}"
  CANDIDATE_BUILD="${CANDIDATE_VERSION#0.}"
  if (( CANDIDATE_BUILD != CURRENT_BUILD && CANDIDATE_BUILD != CURRENT_BUILD + 1 )); then
    echo "$RESUME_TAG cannot be resumed from VERSION $CURRENT_VERSION." >&2
    exit 2
  fi
else
  NEXT_BUILD="$(( ${CURRENT_VERSION#0.} + 1 ))"
  CANDIDATE_VERSION="0.$NEXT_BUILD"
  RESUMING=0
  if [[ -n "$REQUESTED_VERSION" && "$REQUESTED_VERSION" != "$CANDIDATE_VERSION" ]]; then
    echo "The next sequential version is $CANDIDATE_VERSION, not $REQUESTED_VERSION." >&2
    exit 2
  fi
fi

BUILD_VERSION="${CANDIDATE_VERSION#0.}"
TAG="v$CANDIDATE_VERSION"
OUTPUT_DIR="target/release/releases/$TAG"
APP_PATH="$OUTPUT_DIR/Choro.app"
NOTARY_ZIP="$OUTPUT_DIR/Choro-$CANDIDATE_VERSION-notary.zip"
UPDATER_ZIP="$OUTPUT_DIR/Choro-$CANDIDATE_VERSION-arm64.zip"
DMG_STAGING="$OUTPUT_DIR/dmg-staging"
DMG_PATH="$OUTPUT_DIR/Choro-$CANDIDATE_VERSION-arm64.dmg"
CHECKSUMS="$OUTPUT_DIR/SHA256SUMS"
APPCAST="$OUTPUT_DIR/appcast.xml"
NOTES_FILE="$OUTPUT_DIR/release-notes.txt"
STATE_FILE="$OUTPUT_DIR/release-state.json"

if [[ -n "$NOTES_SOURCE" && ! -f "$NOTES_SOURCE" && ! -f "$NOTES_FILE" ]]; then
  echo "Release notes file does not exist: $NOTES_SOURCE" >&2
  exit 1
fi

print_plan() {
  echo "Choro release plan"
  echo "  Source:        $BRANCH @ $(git rev-parse HEAD)"
  echo "  Version:       $CURRENT_VERSION -> $CANDIDATE_VERSION (build $BUILD_VERSION)"
  echo "  Release app:   $APP_PATH"
  echo "  Updater ZIP:   $UPDATER_ZIP"
  echo "  Installer DMG: $DMG_PATH"
  echo "  Checksums:     $CHECKSUMS"
  echo "  Signed feed:   $APPCAST"
  echo "  Apple:         notarize and staple Choro.app and the DMG"
  echo "  Git:           commit 'chore: release $CANDIDATE_VERSION', annotate $TAG, push $BRANCH and $TAG"
  echo "  GitHub:        draft 'Choro $CANDIDATE_VERSION', upload ZIP/DMG/checksums/appcast, validate, publish latest"
  echo "  Feed:          commit 'chore: publish update feed for $CANDIDATE_VERSION' and push $BRANCH"
}

write_state() {
  local stage="$1"
  if [[ -z "$STATE_FILE" || ! -d "$OUTPUT_DIR" || "$DRY_RUN" == "1" ]]; then
    return
  fi
  printf '{\n  "version": "%s",\n  "tag": "%s",\n  "stage": "%s"\n}\n' \
    "$CANDIDATE_VERSION" "$TAG" "$stage" > "$STATE_FILE"
}

on_error() {
  local exit_code=$?
  write_state "failed:$CURRENT_STAGE"
  echo "Release stopped during: $CURRENT_STAGE" >&2
  echo "No tags, releases, or artifacts were deleted. Resume with: ./scripts/release-macos.sh --resume $TAG" >&2
  exit "$exit_code"
}
trap on_error ERR

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Required command is unavailable: $1" >&2
    exit 1
  fi
}

verify_signing_authority() {
  local artifact="$1"
  local signing_details
  signing_details="$(codesign -dv --verbose=4 "$artifact" 2>&1)"
  if ! grep -Fq "Authority=$SIGN_IDENTITY" <<< "$signing_details"; then
    echo "Artifact is not signed by the configured Developer ID identity:" >&2
    echo "  $artifact" >&2
    echo "  Expected: $SIGN_IDENTITY" >&2
    return 1
  fi
}

validate_appcast() {
  node scripts/generate-appcast.mjs \
    --validate "$APPCAST" \
    --version "$CANDIDATE_VERSION" \
    --build "$BUILD_VERSION" \
    --notes-file "$NOTES_FILE" \
    --length "$ARCHIVE_LENGTH" \
    --signature "$ARCHIVE_SIGNATURE" \
    --asset-url "$ASSET_URL"
}

remote_branch_sha() {
  local sha
  sha="$(git ls-remote --heads origin "refs/heads/$BRANCH" | awk 'NR == 1 { print $1 }')"
  if [[ -z "$sha" ]]; then
    echo "Could not resolve origin/$BRANCH." >&2
    return 1
  fi
  printf '%s\n' "$sha"
}

for command in \
  cargo git gh node npm curl ditto codesign security xcrun hdiutil shasum file \
  xmllint spctl cmp readlink mktemp tar rsync find sed awk grep sort head date mv cp ln; do
  require_command "$command"
done
if [[ ! -x /usr/libexec/PlistBuddy ]]; then
  echo "Required Apple tool is unavailable: /usr/libexec/PlistBuddy" >&2
  exit 1
fi

if [[ "$(uname -s)" != "Darwin" || "$(uname -m)" != "arm64" ]]; then
  echo "Choro releases must be built on Apple Silicon macOS." >&2
  exit 1
fi
if [[ "$(git branch --show-current)" != "$BRANCH" ]]; then
  echo "Release from '$BRANCH'; current branch is '$(git branch --show-current)'." >&2
  exit 1
fi
WORKTREE_STATUS="$(git status --porcelain --untracked-files=all)"
if [[ "$RESUMING" == "0" && -n "$WORKTREE_STATUS" ]]; then
  echo "The worktree must be clean before releasing." >&2
  exit 1
elif [[ "$RESUMING" == "1" && -n "$WORKTREE_STATUS" ]]; then
  UNEXPECTED_STATUS="$(printf '%s\n' "$WORKTREE_STATUS" | awk '
    substr($0, 4) != "VERSION" && substr($0, 4) != "release/appcast.xml" { print }
  ')"
  if [[ -n "$UNEXPECTED_STATUS" ]]; then
    echo "Resume found unrelated worktree changes:" >&2
    printf '%s\n' "$UNEXPECTED_STATUS" >&2
    exit 1
  fi
  if printf '%s\n' "$WORKTREE_STATUS" | awk 'substr($0, 4) == "VERSION" { found = 1 } END { exit !found }'; then
    if [[ "$(tr -d '[:space:]' < VERSION)" != "$CANDIDATE_VERSION" ]]; then
      echo "Resume found an unexpected VERSION change; it was left untouched." >&2
      exit 1
    fi
  fi
fi

REMOTE_BRANCH_SHA="$(remote_branch_sha)"
if [[ "$RESUMING" == "0" && "$(git rev-parse HEAD)" != "$REMOTE_BRANCH_SHA" ]]; then
  echo "Local $BRANCH must exactly match origin/$BRANCH." >&2
  exit 1
elif [[ "$RESUMING" == "1" ]]; then
  if ! git merge-base --is-ancestor "$REMOTE_BRANCH_SHA" HEAD; then
    echo "Resume requires local $BRANCH to match or be a release-only fast-forward of origin/$BRANCH." >&2
    exit 1
  fi
  while IFS= read -r SUBJECT; do
    [[ -z "$SUBJECT" ]] && continue
    if [[ "$SUBJECT" != "chore: release $CANDIDATE_VERSION" \
      && "$SUBJECT" != "chore: publish update feed for $CANDIDATE_VERSION" ]]; then
      echo "Resume found an unrelated local commit ahead of origin/$BRANCH: $SUBJECT" >&2
      exit 1
    fi
  done < <(git log --format='%s' "$REMOTE_BRANCH_SHA..HEAD")
fi

gh auth status --hostname github.com >/dev/null
if [[ "$(gh api "repos/$REPOSITORY" --jq '.permissions.admin // false')" != "true" ]]; then
  echo "The active GitHub account cannot publish $REPOSITORY." >&2
  exit 1
fi
if ! security find-identity -v -p codesigning | grep -Fq "$SIGN_IDENTITY"; then
  echo "Developer ID identity is unavailable: $SIGN_IDENTITY" >&2
  exit 1
fi
xcrun notarytool history --keychain-profile "$NOTARY_PROFILE" --output-format json >/dev/null

SPARKLE_ROOT="$(scripts/fetch-sparkle.sh)"
GENERATE_KEYS="$SPARKLE_ROOT/bin/generate_keys"
SIGN_UPDATE="$SPARKLE_ROOT/bin/sign_update"
if ! KEYCHAIN_PUBLIC_KEY="$("$GENERATE_KEYS" --account "$SPARKLE_ACCOUNT" -p)"; then
  echo "Sparkle signing key '$SPARKLE_ACCOUNT' is unavailable in Keychain." >&2
  exit 1
fi
TRACKED_PUBLIC_KEY="$(tr -d '[:space:]' < release/sparkle-public-key.txt)"
KEYCHAIN_PUBLIC_KEY="$(printf '%s' "$KEYCHAIN_PUBLIC_KEY" | tr -d '[:space:]')"
if [[ -z "$TRACKED_PUBLIC_KEY" || "$KEYCHAIN_PUBLIC_KEY" != "$TRACKED_PUBLIC_KEY" ]]; then
  echo "Sparkle Keychain key '$SPARKLE_ACCOUNT' does not match release/sparkle-public-key.txt." >&2
  exit 1
fi

if [[ "$RESUMING" == "0" ]]; then
  if git show-ref --verify --quiet "refs/tags/$TAG" || \
    git ls-remote --exit-code --tags origin "refs/tags/$TAG" >/dev/null 2>&1; then
    echo "Tag already exists: $TAG" >&2
    exit 1
  fi
  if gh release view "$TAG" --repo "$REPOSITORY" >/dev/null 2>&1; then
    echo "GitHub release already exists: $TAG" >&2
    exit 1
  fi
  if [[ -e "$OUTPUT_DIR" ]]; then
    echo "Release output already exists: $OUTPUT_DIR" >&2
    echo "Use --resume $TAG after inspecting it; it was not overwritten." >&2
    exit 1
  fi
else
  if [[ ! -d "$OUTPUT_DIR" ]]; then
    echo "Resume output is missing: $OUTPUT_DIR" >&2
    exit 1
  fi
  if [[ ! -f "$STATE_FILE" ]]; then
    echo "Resume state is missing: $STATE_FILE" >&2
    exit 1
  fi
  node -e '
    const fs = require("node:fs");
    const [file, version, tag] = process.argv.slice(1);
    const state = JSON.parse(fs.readFileSync(file, "utf8"));
    if (state.version !== version || state.tag !== tag || typeof state.stage !== "string") process.exit(1);
  ' "$STATE_FILE" "$CANDIDATE_VERSION" "$TAG" || {
    echo "Resume state does not match $TAG: $STATE_FILE" >&2
    exit 1
  }
  if [[ ! -e "$APP_PATH" ]]; then
    for DOWNSTREAM_PATH in \
      "$NOTARY_ZIP" "$UPDATER_ZIP" "$DMG_STAGING" "$DMG_PATH" \
      "$CHECKSUMS" "$APPCAST" "$NOTES_FILE"; do
      if [[ -e "$DOWNSTREAM_PATH" ]]; then
        echo "Resume is inconsistent: $APP_PATH is missing but a later artifact exists:" >&2
        echo "  $DOWNSTREAM_PATH" >&2
        echo "Nothing was overwritten or removed." >&2
        exit 1
      fi
    done
  fi
fi

if [[ "$DRY_RUN" == "1" ]]; then
  print_plan
  echo "  Signing:       $SIGN_IDENTITY"
  echo "  Notary:        $NOTARY_PROFILE"
  echo "  Mode:          validation only; no build, mutation, notarization, push, or upload"
  exit 0
fi

if [[ "$RESUMING" == "0" || ! -e "$APP_PATH" ]]; then
  mkdir -p "$OUTPUT_DIR"
  write_state "building"
  CURRENT_STAGE="tests"
  cargo test -p ide-core
  cargo test -p ide-app

  CURRENT_STAGE="production workspace build"
  cargo build --release --workspace

  CURRENT_STAGE="bundle"
  PARTIAL_APP="$OUTPUT_DIR/.Choro.app.partial.$$.app"
  CHORO_APP_VERSION="$CANDIDATE_VERSION" \
  CHORO_BUILD_VERSION="$BUILD_VERSION" \
  CHORO_BUNDLE_PATH="$PARTIAL_APP" \
  CHORO_INSTALL_TO_APPLICATIONS=0 \
  CHORO_CODESIGN_IDENTITY="$SIGN_IDENTITY" \
    scripts/bundle.sh
  if [[ -e "$APP_PATH" ]]; then
    echo "Candidate app appeared while building; neither bundle was overwritten:" >&2
    echo "  $APP_PATH" >&2
    echo "  $PARTIAL_APP" >&2
    exit 1
  fi
  mv -n "$PARTIAL_APP" "$APP_PATH"
fi

CURRENT_STAGE="local validation"
[[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$APP_PATH/Contents/Info.plist")" == "$CANDIDATE_VERSION" ]]
[[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleVersion' "$APP_PATH/Contents/Info.plist")" == "$BUILD_VERSION" ]]
file "$APP_PATH/Contents/MacOS/choro" | grep -q 'arm64'
file "$APP_PATH/Contents/MacOS/choro-mcp" | grep -q 'arm64'
file "$APP_PATH/Contents/Frameworks/Chromium Embedded Framework.framework/Chromium Embedded Framework" | grep -q 'arm64'
file "$APP_PATH/Contents/Frameworks/Sparkle.framework/Sparkle" | grep -q 'arm64'
for HELPER_PLIST in "$APP_PATH"/Contents/Frameworks/choro\ Helper*.app/Contents/Info.plist; do
  [[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$HELPER_PLIST")" == "$CANDIDATE_VERSION" ]]
  [[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleVersion' "$HELPER_PLIST")" == "$BUILD_VERSION" ]]
  HELPER_EXECUTABLE="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$HELPER_PLIST")"
  file "${HELPER_PLIST:h}/MacOS/$HELPER_EXECUTABLE" | grep -q 'arm64'
done
codesign --verify --deep --strict --verbose=2 "$APP_PATH"
verify_signing_authority "$APP_PATH"

POST_BUILD_STATUS="$(git status --porcelain --untracked-files=all)"
if [[ "$RESUMING" == "0" && -n "$POST_BUILD_STATUS" ]]; then
  echo "The build changed the source worktree; release was stopped before confirmation:" >&2
  printf '%s\n' "$POST_BUILD_STATUS" >&2
  exit 1
elif [[ "$RESUMING" == "1" && -n "$POST_BUILD_STATUS" ]]; then
  UNEXPECTED_STATUS="$(printf '%s\n' "$POST_BUILD_STATUS" | awk '
    substr($0, 4) != "VERSION" && substr($0, 4) != "release/appcast.xml" { print }
  ')"
  if [[ -n "$UNEXPECTED_STATUS" ]]; then
    echo "The resumed build introduced unrelated worktree changes:" >&2
    printf '%s\n' "$UNEXPECTED_STATUS" >&2
    exit 1
  fi
fi
write_state "awaiting-confirmation"

echo
print_plan
echo
echo "Ready to release Choro $CANDIDATE_VERSION."
echo "This will submit the app and DMG to Apple, commit VERSION, create and push $TAG,"
echo "push $BRANCH, publish a private GitHub release, then update release/appcast.xml."
echo "No existing files, tags, releases, or assets will be deleted or overwritten."
read "CONFIRMATION?Type $TAG to continue: "
if [[ "$CONFIRMATION" != "$TAG" ]]; then
  echo "Release cancelled. Candidate artifacts remain at $OUTPUT_DIR."
  write_state "cancelled"
  exit 0
fi

CURRENT_STAGE="app notarization"
write_state "notarizing-app"
if ! xcrun stapler validate "$APP_PATH" >/dev/null 2>&1; then
  if [[ ! -e "$NOTARY_ZIP" ]]; then
    PARTIAL_NOTARY_ZIP="$OUTPUT_DIR/.Choro-$CANDIDATE_VERSION-notary.partial.$$.zip"
    ditto -c -k --sequesterRsrc --keepParent "$APP_PATH" "$PARTIAL_NOTARY_ZIP"
    [[ ! -e "$NOTARY_ZIP" ]]
    mv -n "$PARTIAL_NOTARY_ZIP" "$NOTARY_ZIP"
  fi
  xcrun notarytool submit "$NOTARY_ZIP" --keychain-profile "$NOTARY_PROFILE" --wait
  xcrun stapler staple "$APP_PATH"
fi
xcrun stapler validate "$APP_PATH"
spctl --assess --type execute --verbose=2 "$APP_PATH"

CURRENT_STAGE="packaging"
write_state "packaging"
if [[ ! -e "$UPDATER_ZIP" ]]; then
  PARTIAL_UPDATER_ZIP="$OUTPUT_DIR/.Choro-$CANDIDATE_VERSION-arm64.partial.$$.zip"
  ditto -c -k --sequesterRsrc --keepParent "$APP_PATH" "$PARTIAL_UPDATER_ZIP"
  [[ ! -e "$UPDATER_ZIP" ]]
  mv -n "$PARTIAL_UPDATER_ZIP" "$UPDATER_ZIP"
fi
ARCHIVE_SIGNATURE_OUTPUT="$("$SIGN_UPDATE" --account "$SPARKLE_ACCOUNT" "$UPDATER_ZIP")"
ARCHIVE_SIGNATURE="$(printf '%s\n' "$ARCHIVE_SIGNATURE_OUTPUT" | sed -n 's/.*sparkle:edSignature="\([^"]*\)".*/\1/p')"
ARCHIVE_LENGTH="$(printf '%s\n' "$ARCHIVE_SIGNATURE_OUTPUT" | sed -n 's/.*length="\([0-9]*\)".*/\1/p')"
if [[ -z "$ARCHIVE_SIGNATURE" || -z "$ARCHIVE_LENGTH" ]]; then
  echo "Sparkle did not return an archive signature and length." >&2
  exit 1
fi
"$SIGN_UPDATE" --account "$SPARKLE_ACCOUNT" --verify "$UPDATER_ZIP" "$ARCHIVE_SIGNATURE"

if [[ ! -e "$DMG_STAGING" ]]; then
  PARTIAL_DMG_STAGING="$OUTPUT_DIR/.dmg-staging.partial.$$"
  mkdir -p "$PARTIAL_DMG_STAGING"
  ditto "$APP_PATH" "$PARTIAL_DMG_STAGING/Choro.app"
  ln -s /Applications "$PARTIAL_DMG_STAGING/Applications"
  [[ ! -e "$DMG_STAGING" ]]
  mv -n "$PARTIAL_DMG_STAGING" "$DMG_STAGING"
fi
if [[ ! -d "$DMG_STAGING/Choro.app" || ! -L "$DMG_STAGING/Applications" \
  || "$(readlink "$DMG_STAGING/Applications")" != "/Applications" ]]; then
  echo "DMG staging is incomplete and was left in place: $DMG_STAGING" >&2
  exit 1
fi
if [[ ! -e "$DMG_PATH" ]]; then
  PARTIAL_DMG="$OUTPUT_DIR/.Choro-$CANDIDATE_VERSION-arm64.partial.$$.dmg"
  hdiutil create -volname "Choro $CANDIDATE_VERSION" -srcfolder "$DMG_STAGING" -format UDZO "$PARTIAL_DMG"
  [[ ! -e "$DMG_PATH" ]]
  mv -n "$PARTIAL_DMG" "$DMG_PATH"
fi
if ! codesign --verify --verbose=2 "$DMG_PATH" >/dev/null 2>&1; then
  codesign --force --timestamp --sign "$SIGN_IDENTITY" "$DMG_PATH"
fi
verify_signing_authority "$DMG_PATH"

CURRENT_STAGE="dmg notarization"
if ! xcrun stapler validate "$DMG_PATH" >/dev/null 2>&1; then
  xcrun notarytool submit "$DMG_PATH" --keychain-profile "$NOTARY_PROFILE" --wait
  xcrun stapler staple "$DMG_PATH"
fi
xcrun stapler validate "$DMG_PATH"
spctl --assess --type open --context context:primary-signature --verbose=2 "$DMG_PATH"

if [[ ! -e "$CHECKSUMS" ]]; then
  PARTIAL_CHECKSUMS="$OUTPUT_DIR/.SHA256SUMS.partial.$$"
  (
    cd "$OUTPUT_DIR"
    shasum -a 256 "${UPDATER_ZIP:t}" "${DMG_PATH:t}" > "${PARTIAL_CHECKSUMS:t}"
  )
  [[ ! -e "$CHECKSUMS" ]]
  mv -n "$PARTIAL_CHECKSUMS" "$CHECKSUMS"
fi

if [[ ! -e "$NOTES_FILE" ]]; then
  PARTIAL_NOTES="$OUTPUT_DIR/.release-notes.partial.$$"
  if [[ -n "$NOTES_SOURCE" ]]; then
    cp "$NOTES_SOURCE" "$PARTIAL_NOTES"
  else
    PREVIOUS_TAG="$(git describe --tags --abbrev=0 2>/dev/null || true)"
    if [[ -n "$PREVIOUS_TAG" ]]; then
      git log --first-parent --pretty='- %s' "$PREVIOUS_TAG..HEAD" > "$PARTIAL_NOTES"
    else
      printf 'First signed and notarized Choro release with in-app updates.\n\nSource: %s\n' \
        "$(git rev-parse --short HEAD)" > "$PARTIAL_NOTES"
    fi
  fi
  [[ ! -e "$NOTES_FILE" ]]
  mv -n "$PARTIAL_NOTES" "$NOTES_FILE"
fi

CURRENT_STAGE="git release identity"
write_state "publishing-git"
if ! git show-ref --verify --quiet "refs/tags/$TAG"; then
  if [[ "$(git show HEAD:VERSION 2>/dev/null || true)" == "$CANDIDATE_VERSION" \
    && "$(git show -s --format='%s' HEAD)" == "chore: release $CANDIDATE_VERSION" ]]; then
    RELEASE_COMMIT="$(git rev-parse HEAD)"
  else
    printf '%s\n' "$CANDIDATE_VERSION" > VERSION
    git add VERSION
    if git diff --cached --quiet -- VERSION; then
      echo "VERSION has no release change to commit for $CANDIDATE_VERSION." >&2
      exit 1
    fi
    git commit -m "chore: release $CANDIDATE_VERSION"
    RELEASE_COMMIT="$(git rev-parse HEAD)"
  fi
  git tag -a "$TAG" -m "Choro $CANDIDATE_VERSION"
else
  if [[ "$(git cat-file -t "$TAG")" != "tag" ]]; then
    echo "Existing tag is not annotated: $TAG" >&2
    exit 1
  fi
  RELEASE_COMMIT="$(git rev-list -n 1 "$TAG")"
  [[ "$(git show "${TAG}:VERSION")" == "$CANDIDATE_VERSION" ]]
  [[ "$(git show -s --format='%s' "$RELEASE_COMMIT")" == "chore: release $CANDIDATE_VERSION" ]]
  git merge-base --is-ancestor "$RELEASE_COMMIT" HEAD
fi

[[ "$(tr -d '[:space:]' < VERSION)" == "$CANDIDATE_VERSION" ]]
REMOTE_BRANCH_SHA="$(remote_branch_sha)"
if [[ "$(git rev-parse HEAD)" != "$REMOTE_BRANCH_SHA" ]]; then
  git push origin "$BRANCH"
fi
[[ "$(git rev-parse HEAD)" == "$(remote_branch_sha)" ]]
REMOTE_TAG_OBJECT="$(git ls-remote --tags origin "refs/tags/$TAG" | awk 'NR == 1 { print $1 }')"
REMOTE_TAG_COMMIT="$(git ls-remote --tags origin "refs/tags/$TAG^{}" | awk 'NR == 1 { print $1 }')"
if [[ -z "$REMOTE_TAG_OBJECT" ]]; then
  git push origin "$TAG"
elif [[ -z "$REMOTE_TAG_COMMIT" || "$REMOTE_TAG_COMMIT" != "$RELEASE_COMMIT" ]]; then
  echo "Remote tag $TAG does not match the verified local annotated tag." >&2
  exit 1
fi

CURRENT_STAGE="GitHub draft"
write_state "uploading-release"
if ! gh release view "$TAG" --repo "$REPOSITORY" >/dev/null 2>&1; then
  gh release create "$TAG" --repo "$REPOSITORY" --verify-tag --draft \
    --title "Choro $CANDIDATE_VERSION" --notes-file "$NOTES_FILE"
fi

for ASSET in "$UPDATER_ZIP" "$DMG_PATH" "$CHECKSUMS"; do
  ASSET_NAME="${ASSET:t}"
  if gh release view "$TAG" --repo "$REPOSITORY" --json assets \
    --jq ".assets[].name" | grep -Fxq "$ASSET_NAME"; then
    echo "Release asset already exists; leaving it unchanged: $ASSET_NAME"
  else
    gh release upload "$TAG" "$ASSET" --repo "$REPOSITORY"
  fi
done

RELEASE_ID="$(gh api "repos/$REPOSITORY/releases" --paginate \
  --jq ".[] | select(.tag_name == \"$TAG\") | .id" | head -n 1)"
ZIP_ASSET_ID="$(gh api "repos/$REPOSITORY/releases/$RELEASE_ID/assets" --paginate \
  --jq ".[] | select(.name == \"${UPDATER_ZIP:t}\") | .id" | head -n 1)"
if [[ -z "$RELEASE_ID" || -z "$ZIP_ASSET_ID" ]]; then
  echo "Could not resolve the uploaded updater archive asset ID." >&2
  exit 1
fi
ASSET_URL="https://api.github.com/repos/$REPOSITORY/releases/assets/$ZIP_ASSET_ID"

CURRENT_STAGE="signed appcast"
if [[ ! -e "$APPCAST" ]]; then
  PARTIAL_APPCAST="$OUTPUT_DIR/.appcast.partial.$$.xml"
  node scripts/generate-appcast.mjs \
    --version "$CANDIDATE_VERSION" \
    --build "$BUILD_VERSION" \
    --notes-file "$NOTES_FILE" \
    --length "$ARCHIVE_LENGTH" \
    --signature "$ARCHIVE_SIGNATURE" \
    --asset-url "$ASSET_URL" \
    --pub-date "$(LC_ALL=C date -R)" \
    --output "$PARTIAL_APPCAST"
  "$SIGN_UPDATE" --account "$SPARKLE_ACCOUNT" "$PARTIAL_APPCAST"
  "$SIGN_UPDATE" --account "$SPARKLE_ACCOUNT" --verify "$PARTIAL_APPCAST"
  [[ ! -e "$APPCAST" ]]
  mv -n "$PARTIAL_APPCAST" "$APPCAST"
fi
xmllint --noout "$APPCAST"
"$SIGN_UPDATE" --account "$SPARKLE_ACCOUNT" --verify "$APPCAST"
validate_appcast
if ! grep -Fq "$ASSET_URL" "$APPCAST"; then
  echo "Appcast does not point at the private GitHub asset API URL." >&2
  exit 1
fi
if grep -Eq 'github\.com/.*/releases/(download|tag)/' "$APPCAST"; then
  echo "Appcast contains a browser release URL instead of only the private asset API URL." >&2
  exit 1
fi

if gh release view "$TAG" --repo "$REPOSITORY" --json assets \
  --jq '.assets[].name' | grep -Fxq 'appcast.xml'; then
  echo "Release asset already exists; leaving it unchanged: appcast.xml"
else
  gh release upload "$TAG" "$APPCAST" --repo "$REPOSITORY"
fi

EXPECTED_ASSETS="$(printf '%s\n' "${UPDATER_ZIP:t}" "${DMG_PATH:t}" "${CHECKSUMS:t}" 'appcast.xml' | sort)"
ACTUAL_ASSETS="$(gh release view "$TAG" --repo "$REPOSITORY" --json assets --jq '.assets[].name' | sort)"
if [[ "$ACTUAL_ASSETS" != "$EXPECTED_ASSETS" ]]; then
  echo "GitHub release assets do not exactly match the expected four files." >&2
  echo "Expected:" >&2
  printf '%s\n' "$EXPECTED_ASSETS" >&2
  echo "Actual:" >&2
  printf '%s\n' "$ACTUAL_ASSETS" >&2
  exit 1
fi

CURRENT_STAGE="release validation"
mkdir -p "$OUTPUT_DIR/validation-downloads"
for ASSET_NAME in "${UPDATER_ZIP:t}" "${DMG_PATH:t}" "${CHECKSUMS:t}" "appcast.xml"; do
  DESTINATION="$OUTPUT_DIR/validation-downloads/$ASSET_NAME"
  if [[ ! -e "$DESTINATION" ]]; then
    ASSET_ID="$(gh api "repos/$REPOSITORY/releases/$RELEASE_ID/assets" --paginate \
      --jq ".[] | select(.name == \"$ASSET_NAME\") | .id" | head -n 1)"
    if [[ -z "$ASSET_ID" ]]; then
      echo "Could not resolve release asset: $ASSET_NAME" >&2
      exit 1
    fi
    PARTIAL_DOWNLOAD="$(mktemp "$OUTPUT_DIR/validation-downloads/.$ASSET_NAME.partial.XXXXXX")"
    gh api "repos/$REPOSITORY/releases/assets/$ASSET_ID" \
      -H 'Accept: application/octet-stream' > "$PARTIAL_DOWNLOAD"
    [[ ! -e "$DESTINATION" ]]
    mv -n "$PARTIAL_DOWNLOAD" "$DESTINATION"
  fi
done
(
  cd "$OUTPUT_DIR/validation-downloads"
  shasum -a 256 -c "${CHECKSUMS:t}"
)
"$SIGN_UPDATE" --account "$SPARKLE_ACCOUNT" --verify \
  "$OUTPUT_DIR/validation-downloads/${UPDATER_ZIP:t}" "$ARCHIVE_SIGNATURE"
"$SIGN_UPDATE" --account "$SPARKLE_ACCOUNT" --verify \
  "$OUTPUT_DIR/validation-downloads/appcast.xml"
cmp "$CHECKSUMS" "$OUTPUT_DIR/validation-downloads/${CHECKSUMS:t}"
cmp "$APPCAST" "$OUTPUT_DIR/validation-downloads/appcast.xml"
codesign --verify --verbose=2 "$OUTPUT_DIR/validation-downloads/${DMG_PATH:t}"
verify_signing_authority "$OUTPUT_DIR/validation-downloads/${DMG_PATH:t}"
xcrun stapler validate "$OUTPUT_DIR/validation-downloads/${DMG_PATH:t}"
spctl --assess --type open --context context:primary-signature --verbose=2 \
  "$OUTPUT_DIR/validation-downloads/${DMG_PATH:t}"

EXTRACTED_UPDATE="$OUTPUT_DIR/validation-downloads/extracted-updater"
if [[ ! -e "$EXTRACTED_UPDATE" ]]; then
  mkdir -p "$EXTRACTED_UPDATE"
  ditto -x -k "$OUTPUT_DIR/validation-downloads/${UPDATER_ZIP:t}" "$EXTRACTED_UPDATE"
fi
EXTRACTED_APP="$EXTRACTED_UPDATE/Choro.app"
if [[ ! -d "$EXTRACTED_APP" ]]; then
  echo "Downloaded updater ZIP does not contain Choro.app." >&2
  exit 1
fi
[[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$EXTRACTED_APP/Contents/Info.plist")" == "$CANDIDATE_VERSION" ]]
[[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleVersion' "$EXTRACTED_APP/Contents/Info.plist")" == "$BUILD_VERSION" ]]
file "$EXTRACTED_APP/Contents/MacOS/choro" | grep -q 'arm64'
file "$EXTRACTED_APP/Contents/MacOS/choro-mcp" | grep -q 'arm64'
file "$EXTRACTED_APP/Contents/Frameworks/Chromium Embedded Framework.framework/Chromium Embedded Framework" | grep -q 'arm64'
file "$EXTRACTED_APP/Contents/Frameworks/Sparkle.framework/Sparkle" | grep -q 'arm64'
codesign --verify --deep --strict --verbose=2 "$EXTRACTED_APP"
xcrun stapler validate "$EXTRACTED_APP"
spctl --assess --type execute --verbose=2 "$EXTRACTED_APP"
if [[ "$(readlink "$EXTRACTED_APP/Contents/Frameworks/Sparkle.framework/Versions/Current")" != "B" \
  || "$(readlink "$EXTRACTED_APP/Contents/Frameworks/Sparkle.framework/Sparkle")" != "Versions/Current/Sparkle" ]]; then
  echo "Sparkle framework symlinks were not preserved by the updater ZIP." >&2
  exit 1
fi

IS_DRAFT="$(gh release view "$TAG" --repo "$REPOSITORY" --json isDraft --jq .isDraft)"
if [[ "$IS_DRAFT" == "true" ]]; then
  gh release edit "$TAG" --repo "$REPOSITORY" --draft=false --latest
elif [[ "$(gh api "repos/$REPOSITORY/releases/latest" --jq .tag_name 2>/dev/null || true)" != "$TAG" ]]; then
  gh release edit "$TAG" --repo "$REPOSITORY" --latest
fi
if [[ "$(gh api "repos/$REPOSITORY/releases/latest" --jq .tag_name)" != "$TAG" ]]; then
  echo "Published release is not GitHub's latest stable release: $TAG" >&2
  exit 1
fi

CURRENT_STAGE="update feed"
write_state "publishing-feed"
if ! cmp -s "$APPCAST" release/appcast.xml; then
  if ! git diff --quiet -- release/appcast.xml \
    || ! git diff --cached --quiet -- release/appcast.xml; then
    echo "release/appcast.xml has a different local change; it was not overwritten." >&2
    exit 1
  fi
  cp "$APPCAST" release/appcast.xml
fi
git add release/appcast.xml
if ! git diff --cached --quiet -- release/appcast.xml; then
  git commit -m "chore: publish update feed for $CANDIDATE_VERSION"
fi
REMOTE_BRANCH_SHA="$(remote_branch_sha)"
if [[ "$(git rev-parse HEAD)" != "$REMOTE_BRANCH_SHA" ]]; then
  git push origin "$BRANCH"
fi
[[ "$(git rev-parse HEAD)" == "$(remote_branch_sha)" ]]

FINAL_STATUS="$(git status --porcelain --untracked-files=all)"
if [[ -n "$FINAL_STATUS" ]]; then
  echo "Release finished its external operations but the source worktree is not clean:" >&2
  printf '%s\n' "$FINAL_STATUS" >&2
  exit 1
fi

write_state "complete"
echo "Published Choro $CANDIDATE_VERSION: https://github.com/$REPOSITORY/releases/tag/$TAG"
echo "The release artifacts remain at $OUTPUT_DIR."
