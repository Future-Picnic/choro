#!/usr/bin/env bash
# Shared code checks for local verification and macOS releases.
set -Eeuo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if (( $# > 0 )); then
    printf 'Usage: bash scripts/verify-release.sh\n' >&2
    exit 2
fi
if ! command -v cargo >/dev/null 2>&1; then
    printf 'Error: cargo is not installed or is not available in PATH.\n' >&2
    exit 127
fi

CURRENT_STEP="startup"
STARTED_AT=$SECONDS
on_error() {
    local status=$?
    printf '\nRelease verification failed during: %s\n' "$CURRENT_STEP" >&2
    exit "$status"
}
trap on_error ERR

run_step() {
    local number=$1
    local label=$2
    shift 2
    CURRENT_STEP="$label"
    printf '\n[%s/7] %s\n' "$number" "$label"
    "$@"
}

printf 'Choro release code verification\n'
# Check the default feature set separately: enabling GPUI test support everywhere
# can hide an accidentally ungated UI test in the ordinary app test target.
run_step 1 "Compile app targets with default features" cargo check --locked -p ide-app --all-targets
run_step 2 "Compile UI test targets" cargo check --locked -p ide-app --all-targets --features ui-layout-tests
run_step 3 "Run ide-core tests" cargo test --locked -p ide-core
run_step 4 "Run ide-app tests with default features" cargo test --locked -p ide-app
run_step 5 "Run ide-mcp tests" cargo test --locked -p ide-mcp
run_step 6 "Run ide-app UI tests" cargo test --locked -p ide-app --features ui-layout-tests
run_step 7 "Build production workspace" cargo build --locked --release --workspace

CURRENT_STEP="complete"
printf '\nRelease code verification passed in %ss.\n' "$((SECONDS - STARTED_AT))"
