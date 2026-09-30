#!/bin/bash
# The mobile source and release implementation live in the private sibling repo.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
MOBILE_ROOT="${CHORO_MOBILE_ROOT:-$SCRIPT_DIR/../../choro-relay/apps/choro-remote}"
if [[ ! -f "$MOBILE_ROOT/scripts/release_ios.py" ]]; then
  echo "Mobile release script not found. Set CHORO_MOBILE_ROOT to apps/choro-remote in your choro-relay checkout." >&2
  exit 1
fi
exec python3 "$MOBILE_ROOT/scripts/release_ios.py" "$@"
