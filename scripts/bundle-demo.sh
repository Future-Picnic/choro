#!/bin/zsh
# Builds a production-equivalent Choro Demo.app with isolated, seeded data.
set -euo pipefail

cd "$(dirname "$0")/.."

SOURCE_BUNDLE="target/release/bundle/Choro.app"
APP_NAME="Choro Demo"
BUNDLE="target/release/bundle/$APP_NAME.app"
INSTALL_BUNDLE="/Applications/$APP_NAME.app"
ENTITLEMENTS="scripts/choro.entitlements"
INSTALL_TO_APPLICATIONS="${CHORO_DEMO_INSTALL_TO_APPLICATIONS:-1}"
SIGN_IDENTITY="${CHORO_CODESIGN_IDENTITY:-${MY_IDE_CODESIGN_IDENTITY:-}}"
BUILD_ID="$(date -u +%Y%m%dT%H%M%SZ)-$(git rev-parse --short HEAD)"

CHORO_INSTALL_TO_APPLICATIONS=0 scripts/bundle.sh

rm -rf "$BUNDLE"
ditto "$SOURCE_BUNDLE" "$BUNDLE"
cp crates/ide-app/assets/app-icon/AppIcon-Demo.icns \
  "$BUNDLE/Contents/Resources/AppIcon.icns"

mv "$BUNDLE/Contents/MacOS/choro" "$BUNDLE/Contents/MacOS/choro-bin"
cat > "$BUNDLE/Contents/MacOS/choro-demo" <<'WRAPPER'
#!/bin/zsh
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
RUN_ID="$(date -u +%Y%m%d%H%M%S)-$$"
export CHORO_DEMO=1
export CHORO_DEMO_BUILD_ID="__CHORO_DEMO_BUILD_ID__"
export CHORO_DEMO_RESET=1
export CHORO_DATA_DIR="$HOME/Library/Application Support/com.ritmus.choro.demo"
export CHORO_INSTALLATION_KEYCHAIN_SERVICE="com.ritmus.choro.installation.demo.$RUN_ID"
export CHORO_PENPOT_KEYCHAIN_SERVICE="com.ritmus.choro.penpot.demo.$RUN_ID"

TOKEN_FILE="$HOME/Library/Application Support/com.ritmus.choro.demo-secrets/jira-token"
if [[ -z "${CHORO_DEMO_JIRA_TOKEN:-}" && -f "$TOKEN_FILE" ]]; then
  export CHORO_DEMO_JIRA_TOKEN="$(<"$TOKEN_FILE")"
fi

exec "$HERE/choro-bin" "$@"
WRAPPER
sed -i '' "s/__CHORO_DEMO_BUILD_ID__/$BUILD_ID/" "$BUNDLE/Contents/MacOS/choro-demo"
chmod +x "$BUNDLE/Contents/MacOS/choro-demo"

/usr/libexec/PlistBuddy -c "Set :CFBundleExecutable choro-demo" "$BUNDLE/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleIdentifier com.ritmus.choro.demo" "$BUNDLE/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleName $APP_NAME" "$BUNDLE/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleDisplayName $APP_NAME" "$BUNDLE/Contents/Info.plist"

if [[ -z "$SIGN_IDENTITY" ]]; then
  SIGN_IDENTITY="$(
    security find-identity -v -p codesigning 2>/dev/null \
      | awk -F '"' '/Apple Development/ { print $2; exit }'
  )"
fi

if [[ -n "$SIGN_IDENTITY" ]]; then
  codesign --force --options runtime --timestamp=none \
    --entitlements "$ENTITLEMENTS" \
    --sign "$SIGN_IDENTITY" "$BUNDLE/Contents/MacOS/choro-bin"
  codesign --force --options runtime --timestamp=none \
    --sign "$SIGN_IDENTITY" "$BUNDLE/Contents/MacOS/choro-mcp"
  codesign --force --options runtime --timestamp=none \
    --entitlements "$ENTITLEMENTS" --sign "$SIGN_IDENTITY" "$BUNDLE"
else
  codesign --force --entitlements "$ENTITLEMENTS" \
    --sign - "$BUNDLE/Contents/MacOS/choro-bin"
  codesign --force --sign - "$BUNDLE/Contents/MacOS/choro-mcp"
  codesign --force --entitlements "$ENTITLEMENTS" --sign - "$BUNDLE"
fi
codesign --verify --deep --strict "$BUNDLE"

echo "Bundled: $BUNDLE"
echo "Demo build: $BUILD_ID"

if [[ "$INSTALL_TO_APPLICATIONS" != "0" ]]; then
  rm -rf "$INSTALL_BUNDLE"
  ditto "$BUNDLE" "$INSTALL_BUNDLE"
  echo "Installed: $INSTALL_BUNDLE"
  echo "Run: open \"$INSTALL_BUNDLE\""
else
  echo "Run: open \"$BUNDLE\""
fi
