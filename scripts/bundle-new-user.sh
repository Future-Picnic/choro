#!/bin/zsh
# Builds an isolated Choro bundle that provisions a brand-new managed Design user.
set -euo pipefail

cd "$(dirname "$0")/.."

SOURCE_BUNDLE="target/release/bundle/Choro.app"
APP_NAME="Choro New User"
BUNDLE="target/release/bundle/$APP_NAME.app"
INSTALL_BUNDLE="/Applications/$APP_NAME.app"
ENTITLEMENTS="scripts/choro.entitlements"
INSTALL_TO_APPLICATIONS="${CHORO_NEW_USER_INSTALL_TO_APPLICATIONS:-1}"
SIGN_IDENTITY="${CHORO_CODESIGN_IDENTITY:-${MY_IDE_CODESIGN_IDENTITY:-}}"
INSTANCE_ID="${CHORO_NEW_USER_INSTANCE_ID:-$(date -u +%Y%m%d%H%M%S)-${RANDOM}${RANDOM}}"

CHORO_INSTALL_TO_APPLICATIONS=0 scripts/bundle.sh

rm -rf "$BUNDLE"
ditto "$SOURCE_BUNDLE" "$BUNDLE"
cp crates/ide-app/assets/app-icon/AppIcon-NewUser.icns \
  "$BUNDLE/Contents/Resources/AppIcon.icns"

mv "$BUNDLE/Contents/MacOS/choro" "$BUNDLE/Contents/MacOS/choro-bin"
cat > "$BUNDLE/Contents/MacOS/choro-new-user" <<'WRAPPER'
#!/bin/zsh
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
export CHORO_DATA_DIR="$HOME/Library/Application Support/com.ritmus.choro.new-user/__INSTANCE_ID__"
export CHORO_INSTALLATION_KEYCHAIN_SERVICE="com.ritmus.choro.installation.new-user.__INSTANCE_ID__"
export CHORO_PENPOT_KEYCHAIN_SERVICE="com.ritmus.choro.penpot.new-user.__INSTANCE_ID__"

# The unique, empty data directory intentionally exercises the same automatic
# onboarding and managed Design provisioning path as a real first installation.
exec "$HERE/choro-bin" "$@"
WRAPPER
sed -i '' "s/__INSTANCE_ID__/$INSTANCE_ID/g" "$BUNDLE/Contents/MacOS/choro-new-user"
chmod +x "$BUNDLE/Contents/MacOS/choro-new-user"

/usr/libexec/PlistBuddy -c "Set :CFBundleExecutable choro-new-user" "$BUNDLE/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleIdentifier com.ritmus.choro.new-user.$INSTANCE_ID" "$BUNDLE/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleName $APP_NAME" "$BUNDLE/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleDisplayName $APP_NAME" "$BUNDLE/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Add :ChoroNewUserInstance string $INSTANCE_ID" "$BUNDLE/Contents/Info.plist"

if [[ -z "$SIGN_IDENTITY" ]]; then
  SIGN_IDENTITY="$(
    security find-identity -v -p codesigning 2>/dev/null \
      | awk -F '"' '/Apple Development/ { print $2; exit }'
  )"
fi

if [[ -n "$SIGN_IDENTITY" ]]; then
  # `choro-bin` was the signed main executable in the source bundle. Renaming
  # it and replacing CFBundleExecutable turns it into nested code, so seal the
  # Mach-O binaries again before signing the modified outer bundle.
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
echo "New Design user instance: $INSTANCE_ID"

if [[ "$INSTALL_TO_APPLICATIONS" != "0" ]]; then
  rm -rf "$INSTALL_BUNDLE"
  ditto "$BUNDLE" "$INSTALL_BUNDLE"
  echo "Installed: $INSTALL_BUNDLE"
  echo "Run: open \"$INSTALL_BUNDLE\""
else
  echo "Run: open \"$BUNDLE\""
fi
