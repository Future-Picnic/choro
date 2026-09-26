#!/bin/zsh
# Build a persistent, isolated Choro Recording.app for marketing captures.
# It seeds the fictional demo workspace once, then keeps any prepared scenes
# across launches. Never touches Choro.app, Choro Demo.app or their data.
set -euo pipefail
cd "$(dirname "$0")/.."

export RECORDING_BUNDLE="${CHORO_RECORDING_BUNDLE_PATH:-$PWD/target/release/bundle/Choro Recording.app}"
export RECORDING_DATA="$HOME/Library/Application Support/com.ritmus.choro.recording"
export RECORDING_LOG="/tmp/choro-recording.log"
export RECORDING_BUILD_ID="$(date -u +%Y%m%dT%H%M%SZ)-$(git rev-parse --short HEAD)"

CHORO_INSTALL_TO_APPLICATIONS=0 \
CHORO_BUNDLE_PATH="$RECORDING_BUNDLE" \
CHORO_REPLACE_BUNDLE=1 \
scripts/bundle.sh

python3 - <<'PY'
import os
import plistlib
import shlex
import shutil
from pathlib import Path

bundle = Path(os.environ['RECORDING_BUNDLE'])
data = os.environ['RECORDING_DATA']
log = os.environ['RECORDING_LOG']
build_id = os.environ['RECORDING_BUILD_ID']
plist = bundle / 'Contents/Info.plist'
with plist.open('rb') as source:
    info = plistlib.load(source)
info.update(
    CFBundleExecutable='choro-recording',
    CFBundleIdentifier='com.ritmus.choro.recording',
    CFBundleName='Choro Recording',
    CFBundleDisplayName='Choro Recording',
    SUEnableAutomaticChecks=False,
    SUAutomaticallyUpdate=False,
)
with plist.open('wb') as target:
    plistlib.dump(info, target)

environment = {
    'CHORO_DATA_DIR': data,
    'CHORO_RELAY_URL': 'disabled',
    'CHORO_REMOTE_ADDR': '127.0.0.1:0',
    'CHORO_INSTALLATION_KEYCHAIN_SERVICE': 'com.ritmus.choro.installation.recording',
    'CHORO_RELAY_KEYCHAIN_SERVICE': 'com.ritmus.choro.relay.recording',
}
wrapper = bundle / 'Contents/MacOS/choro-recording'
lines = ['#!/bin/zsh', 'set -euo pipefail', 'HERE="$(cd "$(dirname "$0")" && pwd)"']
lines.extend(f'export {key}={shlex.quote(value)}' for key, value in environment.items())
lines.extend([
    'unset CHORO_DEMO_RESET CHORO_DEMO CHORO_DEMO_JIRA_TOKEN CHORO_DEMO_JIRA_BOARD_ID',
    # Seed only on first launch; prepared recording scenes must survive relaunches.
    'if [[ ! -f "$CHORO_DATA_DIR/demo-build-id" ]]; then',
    '  if [[ -e "$CHORO_DATA_DIR" ]]; then',
    '    print -u2 "Recording initialization stopped: existing data will not be reset."',
    '    exit 1',
    '  fi',
    '  export CHORO_DEMO=1',
    f'  export CHORO_DEMO_BUILD_ID={shlex.quote(build_id)}',
    'fi',
    f'exec "$HERE/choro" "$@" >> {shlex.quote(log)} 2>&1',
])
wrapper.write_text('\n'.join(lines) + '\n')
wrapper.chmod(0o755)
shutil.copyfile('crates/ide-app/assets/app-icon/AppIcon-Demo.icns', bundle / 'Contents/Resources/AppIcon.icns')
PY

source scripts/resolve-codesign-identity.zsh
RECORDING_SIGN_IDENTITY="$(resolve_choro_codesign_identity)"
RECORDING_ENTITLEMENTS="scripts/choro.entitlements"
[[ "$RECORDING_SIGN_IDENTITY" != "-" ]] || RECORDING_ENTITLEMENTS="scripts/choro-local.entitlements"
codesign --force --options runtime --timestamp=none --entitlements "$RECORDING_ENTITLEMENTS" \
  --sign "$RECORDING_SIGN_IDENTITY" "$RECORDING_BUNDLE/Contents/MacOS/choro"
codesign --force --options runtime --timestamp=none --entitlements "$RECORDING_ENTITLEMENTS" \
  --sign "$RECORDING_SIGN_IDENTITY" "$RECORDING_BUNDLE"
codesign --verify --deep --strict "$RECORDING_BUNDLE"
print -r -- "Recording app: $RECORDING_BUNDLE" "Data: $RECORDING_DATA" "Log: $RECORDING_LOG"
