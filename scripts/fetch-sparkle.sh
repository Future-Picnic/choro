#!/bin/zsh
set -euo pipefail

cd "$(dirname "$0")/.."

SPARKLE_VERSION="2.9.6"
SPARKLE_SHA256="52bf9e88cdd972fc0c81501377a880e90d47031bd8ca5462488f843e2609e192"
SPARKLE_URL="https://github.com/sparkle-project/Sparkle/releases/download/${SPARKLE_VERSION}/Sparkle-${SPARKLE_VERSION}.tar.xz"
CACHE_PARENT="target/tooling/sparkle"
CACHE_ROOT="${CACHE_PARENT}/${SPARKLE_VERSION}"
ARCHIVE="${CACHE_ROOT}/Sparkle-${SPARKLE_VERSION}.tar.xz"
FRAMEWORK="${CACHE_ROOT}/Sparkle.framework"

validate_distribution() {
  local root="$1"
  local archive="$root/Sparkle-${SPARKLE_VERSION}.tar.xz"
  local framework="$root/Sparkle.framework"

  if [[ ! -f "$archive" ]]; then
    echo "Sparkle cache has no pinned archive: $archive" >&2
    exit 1
  fi
  local actual_sha256
  actual_sha256="$(shasum -a 256 "$archive" | awk '{print $1}')"
  if [[ "$actual_sha256" != "$SPARKLE_SHA256" ]]; then
    echo "Sparkle archive checksum mismatch: $archive" >&2
    echo "Expected: $SPARKLE_SHA256" >&2
    echo "Actual:   $actual_sha256" >&2
    echo "The cache was left in place for inspection; Choro did not delete it." >&2
    exit 1
  fi

  for required in \
    "$framework/Versions/B/Sparkle" \
    "$framework/Versions/B/Autoupdate" \
    "$framework/Versions/B/Updater.app" \
    "$framework/Versions/B/XPCServices/Installer.xpc" \
    "$framework/Versions/B/XPCServices/Downloader.xpc" \
    "$root/bin/generate_keys" \
    "$root/bin/generate_appcast" \
    "$root/bin/sign_update"; do
    if [[ ! -e "$required" ]]; then
      echo "Sparkle distribution is incomplete: $required" >&2
      return 1
    fi
  done
}

if [[ -e "$CACHE_ROOT" ]]; then
  validate_distribution "$CACHE_ROOT"
  echo "$CACHE_ROOT"
  exit 0
fi

mkdir -p "$CACHE_PARENT"
STAGING_ROOT="$(mktemp -d "$CACHE_PARENT/.Sparkle-${SPARKLE_VERSION}.partial.XXXXXX")"
STAGING_ARCHIVE="$STAGING_ROOT/Sparkle-${SPARKLE_VERSION}.tar.xz"
echo "Downloading Sparkle into atomic staging directory: $STAGING_ROOT" >&2
curl --fail --location --retry 3 --output "$STAGING_ARCHIVE" "$SPARKLE_URL"
STAGING_SHA256="$(shasum -a 256 "$STAGING_ARCHIVE" | awk '{print $1}')"
if [[ "$STAGING_SHA256" != "$SPARKLE_SHA256" ]]; then
  echo "Sparkle download checksum mismatch; staging was left for inspection:" >&2
  echo "  $STAGING_ROOT" >&2
  exit 1
fi
tar -xJf "$STAGING_ARCHIVE" -C "$STAGING_ROOT"
validate_distribution "$STAGING_ROOT"

if [[ -e "$CACHE_ROOT" ]]; then
  echo "Sparkle cache appeared while staging; neither directory was overwritten:" >&2
  echo "  $CACHE_ROOT" >&2
  echo "  $STAGING_ROOT" >&2
  exit 1
fi
mv -n "$STAGING_ROOT" "$CACHE_ROOT"
validate_distribution "$CACHE_ROOT"

echo "$CACHE_ROOT"
