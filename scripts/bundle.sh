#!/bin/zsh
# Builds a release binary and wraps it in a minimal macOS .app bundle.
set -euo pipefail

cd "$(dirname "$0")/.."
APP_NAME="Choro"
VERSION_FILE="VERSION"
if [[ ! -f "$VERSION_FILE" ]]; then
  echo "Missing product version source: $VERSION_FILE" >&2
  exit 1
fi
DEFAULT_APP_VERSION="$(tr -d '[:space:]' < "$VERSION_FILE")"
APP_VERSION="${CHORO_APP_VERSION:-$DEFAULT_APP_VERSION}"
if [[ ! "$APP_VERSION" =~ '^0\.[1-9][0-9]*$' ]]; then
  echo "Invalid Choro version '$APP_VERSION'; expected 0.N." >&2
  exit 1
fi
DEFAULT_BUILD_VERSION="${APP_VERSION#0.}"
BUILD_VERSION="${CHORO_BUILD_VERSION:-$DEFAULT_BUILD_VERSION}"
if [[ ! "$BUILD_VERSION" =~ '^[1-9][0-9]*$' ]] || [[ "$BUILD_VERSION" != "$DEFAULT_BUILD_VERSION" ]]; then
  echo "Build version '$BUILD_VERSION' must match product version '$APP_VERSION' (${DEFAULT_BUILD_VERSION})." >&2
  exit 1
fi
BUNDLE="${CHORO_BUNDLE_PATH:-target/release/bundle/$APP_NAME.app}"
INSTALL_BUNDLE="${CHORO_INSTALL_BUNDLE_PATH:-/Applications/$APP_NAME.app}"
ENTITLEMENTS="scripts/choro.entitlements"
LOCAL_ENTITLEMENTS="scripts/choro-local.entitlements"
CEF_JIT_ENTITLEMENTS="scripts/choro-cef-jit.entitlements"
CEF_PLUGIN_ENTITLEMENTS="scripts/choro-cef-plugin.entitlements"
INSTALL_TO_APPLICATIONS="${CHORO_INSTALL_TO_APPLICATIONS:-${MY_IDE_INSTALL_TO_APPLICATIONS:-1}}"
SKIP_DOC_EDITOR_BUILD="${CHORO_SKIP_DOC_EDITOR_BUILD:-0}"
REPLACE_EXISTING_BUNDLE="${CHORO_REPLACE_BUNDLE:-0}"
SPARKLE_ROOT="$(scripts/fetch-sparkle.sh)"
SPARKLE_FRAMEWORK="$SPARKLE_ROOT/Sparkle.framework"
SPARKLE_PUBLIC_KEY_FILE="release/sparkle-public-key.txt"
if [[ ! -f "$SPARKLE_PUBLIC_KEY_FILE" ]]; then
  echo "Missing Sparkle public key: $SPARKLE_PUBLIC_KEY_FILE" >&2
  exit 1
fi
SPARKLE_PUBLIC_KEY="$(tr -d '[:space:]' < "$SPARKLE_PUBLIC_KEY_FILE")"
if [[ -z "$SPARKLE_PUBLIC_KEY" ]]; then
  echo "Sparkle public key is empty: $SPARKLE_PUBLIC_KEY_FILE" >&2
  exit 1
fi
source scripts/resolve-codesign-identity.zsh
SIGN_IDENTITY="$(resolve_choro_codesign_identity)"

# The document editor is compiled into the Rust binary with `include_bytes!`.
# Always refresh it before Cargo so a normal app bundle can never ship a stale
# WebView payload. CI and non-destructive staging may explicitly reuse an
# already-built payload.
if [[ "$SKIP_DOC_EDITOR_BUILD" == "1" ]]; then
  if [[ ! -f crates/ide-app/web/doc-editor/dist/editor.js ]] \
    || [[ ! -f crates/ide-app/web/doc-editor/dist/editor.css ]]; then
    echo "CHORO_SKIP_DOC_EDITOR_BUILD=1 requires an existing editor dist." >&2
    exit 1
  fi
else
  (
    cd crates/ide-app/web/doc-editor
    npm ci --no-audit --no-fund
    npm run build
  )
fi

cargo build --release -p ide-app --bin choro --bin choro-cef-helper
cargo build --release -p ide-mcp

case "$(uname -m)" in
  arm64) CEF_ARCH="aarch64" ;;
  x86_64) CEF_ARCH="x86_64" ;;
  *) echo "Unsupported macOS architecture for Chromium: $(uname -m)" >&2; exit 1 ;;
esac
CEF_ROOTS=(target/release/build/cef-dll-sys-*/out/cef_macos_${CEF_ARCH}(N))
if (( ${#CEF_ROOTS[@]} == 0 )); then
  echo "Cargo built CEF but its runtime directory was not found." >&2
  exit 1
fi
CEF_ROOT="$CEF_ROOTS[1]"
CEF_FRAMEWORK="$CEF_ROOT/Chromium Embedded Framework.framework"
if [[ ! -f "$CEF_FRAMEWORK/Chromium Embedded Framework" ]] \
  || [[ ! -f "$CEF_ROOT/CREDITS.html" ]]; then
  echo "The resolved CEF runtime is incomplete: $CEF_ROOT" >&2
  exit 1
fi

if [[ -e "$BUNDLE" ]]; then
  if [[ "$REPLACE_EXISTING_BUNDLE" != "1" ]]; then
    echo "Bundle output already exists: $BUNDLE" >&2
    echo "Choose a new CHORO_BUNDLE_PATH or explicitly set CHORO_REPLACE_BUNDLE=1." >&2
    exit 1
  fi
  rm -rf "$BUNDLE"
fi
mkdir -p "$BUNDLE/Contents/MacOS" "$BUNDLE/Contents/Resources" "$BUNDLE/Contents/Frameworks"
cp target/release/choro "$BUNDLE/Contents/MacOS/choro"
# First-party MCP server, launched by the app next to the main binary.
cp target/release/choro-mcp "$BUNDLE/Contents/MacOS/choro-mcp"
cp crates/ide-app/assets/app-icon/AppIcon.icns "$BUNDLE/Contents/Resources/AppIcon.icns"

# CEF requires its version-matched framework plus macOS helper app bundles.
# `ditto` preserves the framework's versioned directory symlinks.
ditto "$CEF_FRAMEWORK" \
  "$BUNDLE/Contents/Frameworks/Chromium Embedded Framework.framework"
ditto "$SPARKLE_FRAMEWORK" "$BUNDLE/Contents/Frameworks/Sparkle.framework"
for HELPER_SUFFIX in "Helper (GPU)" "Helper (Renderer)" "Helper (Plugin)" "Helper (Alerts)" "Helper"; do
  HELPER_NAME="choro $HELPER_SUFFIX"
  HELPER_BUNDLE="$BUNDLE/Contents/Frameworks/$HELPER_NAME.app"
  mkdir -p "$HELPER_BUNDLE/Contents/MacOS"
  cp target/release/choro-cef-helper "$HELPER_BUNDLE/Contents/MacOS/$HELPER_NAME"
  HELPER_ID_SUFFIX="${HELPER_SUFFIX:l}"
  HELPER_ID_SUFFIX="${HELPER_ID_SUFFIX// /-}"
  HELPER_ID_SUFFIX="${HELPER_ID_SUFFIX//\(/}"
  HELPER_ID_SUFFIX="${HELPER_ID_SUFFIX//\)/}"
  cat > "$HELPER_BUNDLE/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key><string>$HELPER_NAME</string>
    <key>CFBundleIdentifier</key><string>com.ritmus.myide.$HELPER_ID_SUFFIX</string>
    <key>CFBundleName</key><string>$HELPER_NAME</string>
    <key>CFBundleDisplayName</key><string>$HELPER_NAME</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$APP_VERSION</string>
    <key>CFBundleVersion</key><string>$BUILD_VERSION</string>
    <key>LSUIElement</key><true/>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
    <key>LSEnvironment</key>
    <dict><key>MallocNanoZone</key><string>0</string></dict>
</dict>
</plist>
PLIST
done

mkdir -p "$BUNDLE/Contents/Resources/scripts"
# The running app only needs this runtime capability helper. Development,
# compliance, demo, and packaging scripts must not be shipped in Choro.app.
cp scripts/list-agent-runtime-skills.mjs \
  "$BUNDLE/Contents/Resources/scripts/list-agent-runtime-skills.mjs"
mkdir -p "$BUNDLE/Contents/Resources/agent-chat"
rsync -a --delete crates/ide-app/assets/agent-chat/ "$BUNDLE/Contents/Resources/agent-chat/"
if [[ -f "$BUNDLE/Contents/Resources/agent-chat/package.json" ]] \
  && { [[ ! -d "$BUNDLE/Contents/Resources/agent-chat/node_modules/@anthropic-ai/claude-agent-sdk" ]] \
    || [[ ! -f "$BUNDLE/Contents/Resources/agent-chat/node_modules/serve-sim/dist/serve-sim.js" ]]; }; then
  (
    cd "$BUNDLE/Contents/Resources/agent-chat"
    npm install --omit=dev --no-audit --no-fund
  )
fi

# Collect the root Apache grant, notices, vendored/font license material, and
# version-specific license files for every resolved Rust and agent-bridge Node
# package. Keep this after npm install so the Node inventory matches the bundle.
node scripts/collect-third-party-licenses.mjs \
  --output "$BUNDLE/Contents/Resources/licenses" \
  --agent-node-modules "$BUNDLE/Contents/Resources/agent-chat/node_modules" \
  --editor-node-modules "crates/ide-app/web/doc-editor/node_modules"
mkdir -p "$BUNDLE/Contents/Resources/licenses/chromium"
cp "$CEF_ROOT/CREDITS.html" \
  "$BUNDLE/Contents/Resources/licenses/chromium/CREDITS.html"
mkdir -p "$BUNDLE/Contents/Resources/licenses/sparkle"
cp "$SPARKLE_ROOT/LICENSE" "$BUNDLE/Contents/Resources/licenses/sparkle/LICENSE"

cat > "$BUNDLE/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key><string>choro</string>
    <!-- Keep the original signed identity so macOS preserves the user's
         existing Documents/project-folder privacy grants across the rename. -->
    <key>CFBundleIdentifier</key><string>com.ritmus.myide</string>
    <key>CFBundleName</key><string>Choro</string>
    <key>CFBundleDisplayName</key><string>Choro</string>
    <key>CFBundleIconFile</key><string>AppIcon.icns</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$APP_VERSION</string>
    <key>CFBundleVersion</key><string>$BUILD_VERSION</string>
    <key>SUFeedURL</key>
    <string>https://raw.githubusercontent.com/Future-Pinic/choro/dev/release/appcast.xml</string>
    <key>SUPublicEDKey</key><string>$SPARKLE_PUBLIC_KEY</string>
    <key>SUEnableAutomaticChecks</key><true/>
    <key>SUScheduledCheckInterval</key><integer>3600</integer>
    <key>SUAutomaticallyUpdate</key><false/>
    <key>SUAllowsAutomaticUpdates</key><false/>
    <key>SUVerifyUpdateBeforeExtraction</key><true/>
    <key>SURequireSignedFeed</key><true/>
    <key>SUSignedFeedFailureExpirationInterval</key><integer>0</integer>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
    <key>LSEnvironment</key>
    <dict><key>MallocNanoZone</key><string>0</string></dict>
    <key>NSQuitAlwaysKeepsWindows</key><false/>
    <key>NSUserNotificationAlertStyle</key><string>alert</string>
    <key>NSDocumentsFolderUsageDescription</key>
    <string>Choro needs access to your project folders to load Git, docs, services, and environment files.</string>
    <key>NSAppleEventsUsageDescription</key>
    <string>Choro controls the Spotify desktop app only when you choose a companion playlist.</string>
    <key>NSMicrophoneUsageDescription</key>
    <string>Choro uses the microphone only during Project Talk or composer dictation. Audio is processed locally and is not retained.</string>
    <key>NSSpeechRecognitionUsageDescription</key>
    <string>Choro converts speech into editable text and Project Talk questions using local speech models.</string>
    <key>LSMinimumSystemVersion</key><string>13.0</string>
</dict>
</plist>
PLIST

SIGN_ARGS=(--force --options runtime --sign "$SIGN_IDENTITY")
if [[ "$SIGN_IDENTITY" == "-" ]]; then
  SIGN_ARGS+=(--timestamp=none)
  HOST_ENTITLEMENTS="$LOCAL_ENTITLEMENTS"
else
  # Developer ID distribution requires a trusted timestamp. Keep local ad-hoc
  # builds offline, but let Apple's timestamp service seal release signatures.
  SIGN_ARGS+=(--timestamp)
  HOST_ENTITLEMENTS="$ENTITLEMENTS"
fi

# Sign every nested Mach-O file from the inside out before sealing the app.
# Node dependencies may carry native modules, helpers, or dylibs without an
# executable bit, so inspect file contents instead of relying on permissions.
while IFS= read -r -d '' NESTED_FILE; do
  if [[ "$NESTED_FILE" == "$BUNDLE/Contents/MacOS/choro" ]]; then
    continue
  fi
  if [[ "$NESTED_FILE" == "$BUNDLE/Contents/Frameworks/Sparkle.framework/"* ]]; then
    continue
  fi
  if file -b "$NESTED_FILE" | grep -q 'Mach-O'; then
    codesign "${SIGN_ARGS[@]}" "$NESTED_FILE"
  fi
done < <(find "$BUNDLE/Contents" -type f -print0)

# Sparkle's installer services must be signed in this exact inside-out order.
# Downloader carries upstream entitlements that are required for its XPC work.
SPARKLE_BUNDLE="$BUNDLE/Contents/Frameworks/Sparkle.framework"
codesign "${SIGN_ARGS[@]}" "$SPARKLE_BUNDLE/Versions/B/XPCServices/Installer.xpc"
codesign "${SIGN_ARGS[@]}" --preserve-metadata=entitlements \
  "$SPARKLE_BUNDLE/Versions/B/XPCServices/Downloader.xpc"
codesign "${SIGN_ARGS[@]}" "$SPARKLE_BUNDLE/Versions/B/Autoupdate"
codesign "${SIGN_ARGS[@]}" "$SPARKLE_BUNDLE/Versions/B/Updater.app"
codesign "${SIGN_ARGS[@]}" "$SPARKLE_BUNDLE"

# Seal nested CEF bundles after their binaries, then seal Choro itself. The
# renderer/GPU helpers need JIT under hardened runtime; the optional plugin
# process needs to load browser-provided libraries.
codesign "${SIGN_ARGS[@]}" \
  "$BUNDLE/Contents/Frameworks/Chromium Embedded Framework.framework"
for HELPER_SUFFIX in "Helper (GPU)" "Helper (Renderer)" "Helper (Plugin)" "Helper (Alerts)" "Helper"; do
  HELPER_BUNDLE="$BUNDLE/Contents/Frameworks/choro $HELPER_SUFFIX.app"
  case "$HELPER_SUFFIX" in
    "Helper (GPU)"|"Helper (Renderer)")
      codesign "${SIGN_ARGS[@]}" --entitlements "$CEF_JIT_ENTITLEMENTS" "$HELPER_BUNDLE"
      ;;
    "Helper (Plugin)")
      codesign "${SIGN_ARGS[@]}" --entitlements "$CEF_PLUGIN_ENTITLEMENTS" "$HELPER_BUNDLE"
      ;;
    *)
      codesign "${SIGN_ARGS[@]}" "$HELPER_BUNDLE"
      ;;
  esac
done

codesign "${SIGN_ARGS[@]}" \
  --entitlements "$HOST_ENTITLEMENTS" "$BUNDLE"
codesign --verify --deep --strict --verbose=2 "$BUNDLE"
echo "Signed: $SIGN_IDENTITY"

echo "Bundled: $BUNDLE"

if [[ "$INSTALL_TO_APPLICATIONS" != "0" ]]; then
  if [[ -e "$INSTALL_BUNDLE" ]]; then
    echo "Install destination already exists: $INSTALL_BUNDLE" >&2
    echo "Remove it yourself before installing this bundle." >&2
    exit 1
  fi
  ditto "$BUNDLE" "$INSTALL_BUNDLE"
  echo "Installed: $INSTALL_BUNDLE"
  echo "Run: open \"$INSTALL_BUNDLE\""
else
  echo "Run: open \"$BUNDLE\""
fi
