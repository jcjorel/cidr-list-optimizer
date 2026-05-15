#!/usr/bin/env bash
# Run all CLI integration tests.
# Usage: tests/cli/run_tests.sh

set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
FAILED=0

echo "=== CLI Integration Tests ==="
echo ""

for test_file in "$DIR"/test_*.sh; do
    [ -f "$test_file" ] || continue
    chmod +x "$test_file"
    echo "--- $(basename "$test_file") ---"
    if ! bash "$test_file"; then
        FAILED=1
    fi
    echo ""
done

if [ "$FAILED" -eq 0 ]; then
    echo "All test suites passed."
else
    echo "Some tests FAILED." >&2
    exit 1
fi
