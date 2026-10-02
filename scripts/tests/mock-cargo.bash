# Loaded through BASH_ENV only by verify-release.test.mjs.
cargo() {
    printf 'MOCK_CARGO %s\n' "$*"
    if [[ "$*" == "$CHORO_TEST_FAIL_AT" ]]; then
        return 42
    fi
    return 0
}
