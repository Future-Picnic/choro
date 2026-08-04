#!/bin/zsh
# Builds a release binary and wraps it in a minimal macOS .app bundle.
set -euo pipefail

cd "$(dirname "$0")/.."
APP_NAME="Choro"
BUNDLE="target/release/bundle/$APP_NAME.app"
INSTALL_BUNDLE="/Applications/$APP_NAME.app"
ENTITLEMENTS="scripts/choro.entitlements"
SIGN_IDENTITY="${CHORO_CODESIGN_IDENTITY:-${MY_IDE_CODESIGN_IDENTITY:-}}"
INSTALL_TO_APPLICATIONS="${CHORO_INSTALL_TO_APPLICATIONS:-${MY_IDE_INSTALL_TO_APPLICATIONS:-1}}"

# The document editor is compiled into the Rust binary with `include_bytes!`.
# Always refresh it before Cargo so a normal app bundle can never ship a stale
# WebView payload.
(
  cd crates/ide-app/web/doc-editor
  npm ci --no-audit --no-fund
  npm run build
)

cargo build --release -p ide-app
cargo build --release -p ide-mcp

rm -rf "$BUNDLE"
mkdir -p "$BUNDLE/Contents/MacOS" "$BUNDLE/Contents/Resources"
cp target/release/choro "$BUNDLE/Contents/MacOS/choro"
# First-party MCP server, launched by the app next to the main binary.
cp target/release/choro-mcp "$BUNDLE/Contents/MacOS/choro-mcp"
cp crates/ide-app/assets/app-icon/AppIcon.icns "$BUNDLE/Contents/Resources/AppIcon.icns"

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

cat > "$BUNDLE/Contents/Info.plist" <<'PLIST'
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
    <key>CFBundleShortVersionString</key><string>0.1.0</string>
    <key>CFBundleVersion</key><string>1</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSQuitAlwaysKeepsWindows</key><false/>
    <key>NSUserNotificationAlertStyle</key><string>alert</string>
    <key>NSDocumentsFolderUsageDescription</key>
    <string>Choro needs access to your project folders to load Git, docs, services, and environment files.</string>
    <key>NSMicrophoneUsageDescription</key>
    <string>Choro uses the microphone only during Project Talk or composer dictation. Audio is processed locally and is not retained.</string>
    <key>NSSpeechRecognitionUsageDescription</key>
    <string>Choro converts speech into editable text and Project Talk questions using local speech models.</string>
    <key>LSMinimumSystemVersion</key><string>13.0</string>
</dict>
</plist>
PLIST

if [[ -z "$SIGN_IDENTITY" ]]; then
  SIGN_IDENTITY="$(
    security find-identity -v -p codesigning 2>/dev/null \
      | awk -F '"' '/Apple Development/ { print $2; exit }'
  )"
fi

if [[ -n "$SIGN_IDENTITY" ]]; then
  codesign --force --options runtime --timestamp=none \
    --entitlements "$ENTITLEMENTS" --sign "$SIGN_IDENTITY" "$BUNDLE"
  codesign --verify --deep --strict "$BUNDLE"
  echo "Signed: $SIGN_IDENTITY"
else
  echo "Warning: no Apple Development signing identity found; macOS privacy prompts may repeat for unsigned rebuilds." >&2
fi

echo "Bundled: $BUNDLE"

if [[ "$INSTALL_TO_APPLICATIONS" != "0" ]]; then
  rm -rf "$INSTALL_BUNDLE"
  ditto "$BUNDLE" "$INSTALL_BUNDLE"
  echo "Installed: $INSTALL_BUNDLE"
  echo "Run: open \"$INSTALL_BUNDLE\""
else
  echo "Run: open \"$BUNDLE\""
fi
