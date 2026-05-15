#!/usr/bin/env bash
# Test: over-coverage consistency between percentage-target and integer-target modes.
#
# For each over-coverage budget (0.25% to 10% by 0.25% steps):
#   1. Run with --ipvX-target "over-coverage=Y%" → get entry counts
#   2. Run with --ipvX-target <count> --max-over-coverage -1 → get optimized output
#   3. Assert actual over-coverage from (2) is <= budget Y%

set -euo pipefail
source "$(dirname "$0")/lib.sh"

ensure_binary

INPUT="$FIXTURES/cloudfront_cidrs.txt"

echo "=== test_over_coverage_consistency ==="

# Compute lossless baseline IP counts
LOSSLESS=$("$BIN" "$INPUT")
BASELINE_V4=$(echo "$LOSSLESS" | grep '\.' | count_ipv4_ips)
BASELINE_V6=$(echo "$LOSSLESS" | grep ':' | count_ipv6_ips)

# Sweep from 0.25% to 10.00% in 0.25% steps (i=1..40)
for i in $(seq 1 40); do
    PCT=$(printf "%.2f" "$(echo "scale=2; $i * 0.25" | bc)")

    # Step 1: run with over-coverage target
    OC_OUTPUT=$("$BIN" --ipv4-target "over-coverage=${PCT}%" --ipv6-target "over-coverage=${PCT}%" "$INPUT")
    V4_TARGET=$(echo "$OC_OUTPUT" | grep '\.' | wc -l)
    V6_TARGET=$(echo "$OC_OUTPUT" | grep ':' | wc -l)

    # Step 2: run with integer target (same entry count, no over-coverage cap)
    INT_OUTPUT=$("$BIN" --ipv4-target "$V4_TARGET" --ipv6-target "$V6_TARGET" --max-over-coverage -1 "$INPUT")
    INT_V4_IPS=$(echo "$INT_OUTPUT" | grep '\.' | count_ipv4_ips)
    INT_V6_IPS=$(echo "$INT_OUTPUT" | grep ':' | count_ipv6_ips)

    # Step 3: compute actual over-coverage and assert <= budget
    ACTUAL_V4=$(echo "scale=6; ($INT_V4_IPS - $BASELINE_V4) * 100 / $BASELINE_V4" | bc)
    ACTUAL_V6=$(echo "scale=6; ($INT_V6_IPS - $BASELINE_V6) * 100 / $BASELINE_V6" | bc)

    test_start "over-coverage=${PCT}% → v4=$V4_TARGET,v6=$V6_TARGET entries"
    if assert_le "$ACTUAL_V4" "$PCT" "IPv4 over-coverage ${ACTUAL_V4}% exceeds budget ${PCT}%" &&
       assert_le "$ACTUAL_V6" "$PCT" "IPv6 over-coverage ${ACTUAL_V6}% exceeds budget ${PCT}%"; then
        test_pass
    fi
done

test_summary
