#!/usr/bin/env bash

set -Eeuo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if ! command -v cargo >/dev/null 2>&1; then
    printf 'Error: cargo is not installed or is not available in PATH.\n' >&2
    exit 127
fi

CURRENT_STEP="startup"
STARTED_AT=$SECONDS

on_error() {
    local status=$?
    printf '\nVerification failed during: %s\n' "$CURRENT_STEP" >&2
    exit "$status"
}
trap on_error ERR

run_step() {
    local number=$1
    local label=$2
    shift 2
    CURRENT_STEP="$label"
    printf '\n[%s/6] %s\n' "$number" "$label"
    "$@"
}

printf 'Choro verification\n'
printf 'Workspace: %s\n' "$ROOT_DIR"

run_step 1 "Check Rust formatting" cargo fmt --all -- --check
run_step 2 "Compile every workspace target" cargo check --locked --workspace --all-targets
run_step 3 "Run agent bridge and release script tests" node --test scripts/verify-release.test.mjs crates/ide-app/assets/agent-chat/claude_file_attribution.test.mjs crates/ide-app/assets/agent-chat/claude_delegation.test.mjs crates/ide-app/assets/agent-chat/subscription_usage.test.mjs
run_step 4 "Run Velotype tests" cargo test --locked -p velotype --quiet -- --test-threads=1
run_step 5 "Verify bundled Expert skills and provenance" python3 scripts/verify-expert-catalog.py
run_step 6 "Run shared release code verification" bash scripts/verify-release.sh

CURRENT_STEP="complete"
printf '\nVerification passed in %ss.\n' "$((SECONDS - STARTED_AT))"
