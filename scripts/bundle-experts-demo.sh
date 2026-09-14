#!/bin/zsh
# Build a fresh, persistent Band demo without replacing any existing bundle
# or reading production project data. Uses the normal installed provider auth.
set -euo pipefail
cd "$(dirname "$0")/.."

DEMO_INSTANCE="$(date -u +%Y%m%dT%H%M%SZ)-$RANDOM"
export EXPERTS_DEMO_BUNDLE="$PWD/target/release/bundle/Choro Band Demo $DEMO_INSTANCE.app"
# Unix sockets must fit macOS's short sockaddr_un path limit.
export EXPERTS_DEMO_DATA="/tmp/choro-experts-$DEMO_INSTANCE"
export EXPERTS_DEMO_INSTANCE="$DEMO_INSTANCE"
export EXPERTS_DEMO_LOG="/tmp/choro-experts-$DEMO_INSTANCE.log"
[[ ! -e "$EXPERTS_DEMO_BUNDLE" && ! -e "$EXPERTS_DEMO_DATA" ]]

CHORO_INSTALL_TO_APPLICATIONS=0 \
CHORO_BUNDLE_PATH="$EXPERTS_DEMO_BUNDLE" \
scripts/bundle.sh

python3 - <<'PY'
import os
import plistlib
import shlex
import shutil
from pathlib import Path

bundle = Path(os.environ['EXPERTS_DEMO_BUNDLE'])
instance = os.environ['EXPERTS_DEMO_INSTANCE']
data = os.environ['EXPERTS_DEMO_DATA']
log = os.environ['EXPERTS_DEMO_LOG']
plist = bundle / 'Contents/Info.plist'
with plist.open('rb') as source:
    info = plistlib.load(source)
info.update(
    CFBundleExecutable='choro-experts-demo',
    CFBundleIdentifier=f'com.ritmus.choro.experts-demo.{instance.lower()}',
    CFBundleName='Choro Band Demo',
    CFBundleDisplayName='Choro Band Demo',
    SUEnableAutomaticChecks=False,
    SUAutomaticallyUpdate=False,
)
with plist.open('wb') as target:
    plistlib.dump(info, target)

environment = {
    'CHORO_DATA_DIR': data,
    'CHORO_EXPERTS': '1',
    'CHORO_RELAY_URL': 'disabled',
    'CHORO_REMOTE_ADDR': '127.0.0.1:0',
    'CHORO_INSTALLATION_KEYCHAIN_SERVICE': f'com.ritmus.choro.installation.experts-demo.{instance}',
    'CHORO_PENPOT_KEYCHAIN_SERVICE': f'com.ritmus.choro.penpot.experts-demo.{instance}',
    'CHORO_RELAY_KEYCHAIN_SERVICE': f'com.ritmus.choro.relay.experts-demo.{instance}',
}
wrapper = bundle / 'Contents/MacOS/choro-experts-demo'
lines = ['#!/bin/zsh', 'set -euo pipefail', 'HERE="$(cd "$(dirname "$0")" && pwd)"']
lines.extend(f'export {key}={shlex.quote(value)}' for key, value in environment.items())
lines.extend([
    'unset CHORO_DEMO_RESET CHORO_DEMO CHORO_DEMO_JIRA_TOKEN CHORO_DEMO_JIRA_BOARD_ID',
    'if [[ ! -f "$CHORO_DATA_DIR/demo-build-id" ]]; then',
    '  if [[ -e "$CHORO_DATA_DIR" ]]; then',
    '    print -u2 "Demo initialization stopped: existing data will not be reset."',
    '    exit 1',
    '  fi',
    '  export CHORO_DEMO=1',
    f'  export CHORO_DEMO_BUILD_ID={shlex.quote(instance)}',
    'fi',
    f'exec "$HERE/choro" "$@" >> {shlex.quote(log)} 2>&1',
])
wrapper.write_text('\n'.join(lines) + '\n')
wrapper.chmod(0o755)
shutil.copyfile('crates/ide-app/assets/app-icon/AppIcon-Demo.icns', bundle / 'Contents/Resources/AppIcon.icns')
PY

source scripts/resolve-codesign-identity.zsh
DEMO_SIGN_IDENTITY="$(resolve_choro_codesign_identity)"
DEMO_ENTITLEMENTS="scripts/choro.entitlements"
[[ "$DEMO_SIGN_IDENTITY" != "-" ]] || DEMO_ENTITLEMENTS="scripts/choro-local.entitlements"
codesign --force --options runtime --timestamp=none --entitlements "$DEMO_ENTITLEMENTS" \
  --sign "$DEMO_SIGN_IDENTITY" "$EXPERTS_DEMO_BUNDLE/Contents/MacOS/choro"
codesign --force --options runtime --timestamp=none --entitlements "$DEMO_ENTITLEMENTS" \
  --sign "$DEMO_SIGN_IDENTITY" "$EXPERTS_DEMO_BUNDLE"
codesign --verify --deep --strict "$EXPERTS_DEMO_BUNDLE"
print -r -- "Demo: $EXPERTS_DEMO_BUNDLE" "Data: $EXPERTS_DEMO_DATA" "Log: $EXPERTS_DEMO_LOG"
