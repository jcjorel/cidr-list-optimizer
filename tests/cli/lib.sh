#!/usr/bin/env bash
# Shared helpers for CLI integration tests.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BIN="$REPO_ROOT/target/release/cidr-optimizer"
FIXTURES="$REPO_ROOT/tests/cli/fixtures"

# --- Test bookkeeping ---
_TESTS_RUN=0
_TESTS_PASSED=0
_TESTS_FAILED=0
_CURRENT_TEST=""

test_start() {
    _CURRENT_TEST="$1"
    _TESTS_RUN=$((_TESTS_RUN + 1))
    printf "  %-60s " "$1"
}

test_pass() {
    _TESTS_PASSED=$((_TESTS_PASSED + 1))
    echo "✅"
}

test_fail() {
    _TESTS_FAILED=$((_TESTS_FAILED + 1))
    echo "❌"
    echo "    FAIL: $1" >&2
}

test_summary() {
    echo ""
    echo "Results: $_TESTS_PASSED/$_TESTS_RUN passed, $_TESTS_FAILED failed"
    [ "$_TESTS_FAILED" -eq 0 ]
}

# --- Build ---
ensure_binary() {
    if [ ! -x "$BIN" ]; then
        echo "Building release binary..."
        cargo build --release --manifest-path "$REPO_ROOT/Cargo.toml" >/dev/null 2>&1
    fi
}

# --- IP counting ---

# Count total IPv4 addresses covered by CIDRs (one per line on stdin)
count_ipv4_ips() {
    local total=0
    while IFS= read -r cidr; do
        [ -z "$cidr" ] && continue
        local plen="${cidr##*/}"
        total=$((total + (1 << (32 - plen))))
    done
    echo "$total"
}

# Count total IPv6 addresses covered by CIDRs (one per line on stdin)
# Uses bc for large numbers.
count_ipv6_ips() {
    local total=0
    while IFS= read -r cidr; do
        [ -z "$cidr" ] && continue
        local plen="${cidr##*/}"
        local count
        count=$(echo "2^(128 - $plen)" | bc)
        total=$(echo "$total + $count" | bc)
    done
    echo "$total"
}

# --- Assertions ---

assert_eq() {
    local expected="$1" actual="$2" msg="${3:-values should be equal}"
    if [ "$expected" != "$actual" ]; then
        test_fail "$msg (expected=$expected, actual=$actual)"
        return 1
    fi
}

# Assert actual <= limit (integers or bc-compatible numbers)
assert_le() {
    local actual="$1" limit="$2" msg="${3:-should be <= limit}"
    local ok
    ok=$(echo "$actual <= $limit" | bc)
    if [ "$ok" -ne 1 ]; then
        test_fail "$msg (actual=$actual, limit=$limit)"
        return 1
    fi
}

assert_line_count() {
    local expected="$1" actual="$2" msg="${3:-line count mismatch}"
    if [ "$expected" -ne "$actual" ]; then
        test_fail "$msg (expected=$expected lines, got=$actual)"
        return 1
    fi
}
