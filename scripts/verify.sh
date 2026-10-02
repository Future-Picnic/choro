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
    printf '\n[%s/9] %s\n' "$number" "$label"
    "$@"
}

printf 'Choro verification\n'
printf 'Workspace: %s\n' "$ROOT_DIR"

run_step 1 "Check Rust formatting" cargo fmt --all -- --check
run_step 2 "Compile every workspace target" cargo check --workspace --all-targets
run_step 3 "Run ide-core tests" cargo test -p ide-core --quiet
run_step 4 "Run ide-app tests" cargo test -p ide-app --quiet
run_step 5 "Run ide-mcp tests" cargo test -p ide-mcp --quiet
run_step 6 "Run agent bridge tests" node --test crates/ide-app/assets/agent-chat/claude_file_attribution.test.mjs crates/ide-app/assets/agent-chat/claude_delegation.test.mjs crates/ide-app/assets/agent-chat/subscription_usage.test.mjs
run_step 7 "Run Velotype tests" cargo test -p velotype --quiet -- --test-threads=1
run_step 8 "Verify bundled Expert skills and provenance" python3 scripts/verify-expert-catalog.py
run_step 9 "Build production app and MCP binaries" cargo build --release -p ide-app -p ide-mcp

CURRENT_STEP="complete"
printf '\nVerification passed in %ss.\n' "$((SECONDS - STARTED_AT))"
