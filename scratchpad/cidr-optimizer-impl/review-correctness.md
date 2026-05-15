# Correctness Review of Stress Test Plan

## Critical Issues (would cause false test failures or missed bugs)

### Issue 1: Lossy test does not specify `max_over_coverage_ratio` — assertion may be wrong if ratio is set
- Test: `stress_lossy_deterministic.rs` (all sub-tests)
- Claim: "output ≤ target" is always achievable
- Reality: The optimizer's `optimize_trie()` breaks early if `max_over_coverage_ratio` is exceeded (see `optimizer.rs` line: `if exceeds_ratio(...) { break; }`). If the test sets a ratio constraint, the optimizer may legitimately stop before reaching the target, producing MORE entries than the target. The plan does not explicitly state that `max_over_coverage_ratio` should be `None`.
- Fix: Explicitly state that lossy tests MUST use `max_over_coverage_ratio: None` for the "output ≤ target" assertion to hold. Alternatively, if a ratio is set, the assertion must be `output ≤ target OR over_coverage_ratio ≥ max_ratio`.

### Issue 2: Test 6e "Deep nesting with lossy target=1" — output prefix claim is imprecise
- Test: `stress_adversarial.rs` test 6e
- Claim: "Output = 1 entry covering all inputs (some prefix ≥ /18)"
- Reality: The notation "≥ /18" is ambiguous. In CIDR, a "larger" prefix means a shorter prefix length (covers more). The correct statement is the output prefix_len ≤ 18. However, since all 10000 IPs are within `10.0.0.0/18` AND they span both /19 halves (addresses 0-9999, and 8192 < 10000 so both halves are populated), the trie's lowest common ancestor for all leaves IS the /18 node. The output will be exactly `10.0.0.0/18`, not something shorter. But this depends on `max_over_coverage_ratio` being None — with ratio=None and target=1, the optimizer collapses everything to the LCA which is /18. If the test doesn't set ratio=None, it might not reach target=1.
- Fix: Assert output is exactly `10.0.0.0/18` (prefix_len == 18). Explicitly use `max_over_coverage_ratio: None`. Document that the LCA is /18 because inputs span both /19 halves (offset 8192 < 10000).

### Issue 3: Differential test may fail due to output ordering differences
- Test: `stress_differential_v4v6.rs`
- Claim: "Output prefix sets are identical (our output == ipnet::aggregate output)"
- Reality: `ipnet::Ipv4Net::aggregate()` returns prefixes sorted by network address. Our `optimize()` also sorts output by network address then prefix_len (see `build_result`). However, the comparison must be set-based (or both sorted identically). The plan says "order-independent" comparison but doesn't specify how. If both are sorted the same way, direct Vec comparison works. But `ipnet::aggregate()` sorts by `(network, prefix_len)` ascending — same as our code. This should be fine, but the plan should explicitly state the comparison method.
- Fix: Clarify that comparison is done after sorting both outputs by `(network_addr, prefix_len)`, which both implementations already do. Use direct Vec equality.

### Issue 4: Test 6d (sibling pairs) — cascading merges will reduce output far below 5000
- Test: `stress_adversarial.rs` test 6d
- Claim: "Exactly 5000 /24s (each pair merges)" and "Tests pure sibling merging without cascading"
- Reality: The generation `10.{i/256}.{i%256}.0/25` + `10.{i/256}.{i%256}.128/25` for i=0..4999 produces 5000 /24s at CONTIGUOUS addresses: `10.0.0.0/24`, `10.0.1.0/24`, ..., `10.19.135.0/24`. These /24s are themselves sibling pairs (e.g., `10.0.0.0/24` and `10.0.1.0/24` are siblings because they differ only in bit 23). The lossless sibling merge algorithm cascades: pairs of /24s → /23s, pairs of /23s → /22s, etc. The output will be the minimal prefix decomposition of 5000 contiguous /24s, which is far fewer than 5000 entries. Specifically, 5000 = 4096 + 512 + 256 + 128 + 8 = multiple power-of-2 blocks, yielding ~5-7 output prefixes, NOT 5000.
- Fix: To test "pure sibling merging without cascading", use NON-CONTIGUOUS /24 pairs. For example, use stride-2 /24 indices: pairs at `10.{i*2/256}.{(i*2)%256}.0/25` + `.128/25` for i=0..4999. This produces /24s at `10.0.0.0`, `10.0.2.0`, `10.0.4.0`, ... (every other /24). The resulting /24s are NOT siblings (`.0.0/24` and `.2.0/24` differ in bit 22, not bit 23), so no cascading occurs. Output = 5000.

### Issue 5: Test 1 (sequential) — N=1048576 claim "exactly one /12" requires base alignment verification
- Test: `stress_sequential_v4.rs` test_1m
- Claim: "For N=1048576 (1M): exactly 16 contiguous /16s = one /12"
- Reality: 1048576 = 2^20. Starting from `10.0.0.0` (which is `0x0A000000`). The range covers `10.0.0.0` through `10.15.255.255`. For this to merge into a single /12, the base address must be /12-aligned. `10.0.0.0` in binary: `00001010.00000000.00000000.00000000`. A /12 boundary requires the lower 20 bits to be zero: `10.0.0.0` has lower 20 bits = `0000.00000000.00000000` = 0. ✓ So `10.0.0.0` IS /12-aligned, and 2^20 contiguous addresses from a /12-aligned base = exactly one /12.
- Fix: No fix needed — the claim is correct. But the plan should document WHY it works (base alignment).

## Warnings (potential problems depending on implementation details)

### Warning 1: Differential test with non-canonical inputs may expose `trunc()` timing differences
- Test: `stress_differential_v4v6.rs` — "10K mixed /16, /24, /32 with overlaps"
- Issue: Our code calls `.trunc()` in `partition_with_indices()` before passing to lossless. If the test generates non-canonical inputs (e.g., `10.0.0.1/24` with host bits set), our code truncates them to `10.0.0.0/24`. `ipnet::aggregate()` also truncates internally. However, `ipnet`'s `parse()` for `Ipv4Net` does NOT reject non-canonical forms — it stores them as-is. The `aggregate()` function calls `.trunc()` internally. So both should produce the same result. But if the test constructs `Ipv4Net` via `Ipv4Net::new(addr, len).unwrap()` without truncating, the comparison should still work because both sides truncate.
- Risk: Low. Both implementations normalize host bits.

### Warning 2: Test 5 (lossy) — evenly-spaced /32s may have some sibling pairs after lossless
- Test: `stress_lossy_deterministic.rs`
- Issue: stride = `2^32 / N`. For N=10000, stride = 429496. Address 0: `0 * 429496 = 0`. Address 1: `429496`. These are NOT siblings (differ in many bits). But for certain N values, some pairs might accidentally be siblings. For N=10000 with stride=429496: consecutive addresses differ by 429496, which is much larger than 1, so no /32 sibling pairs exist. After lossless, output = 10000 entries. ✓
- Risk: Low for the specific N values chosen, but the plan should note this assumption.

### Warning 3: Test 2 (sequential IPv6) — N=1048576 claim "exactly 1 entry" needs base alignment check
- Test: `stress_sequential_v6.rs` test_1m
- Issue: N=1048576 = 2^20. Starting from `2001:db8::0`. For this to merge to one /108, the base must be /108-aligned. `2001:db8::0` has the lower 20 bits (bits 108-127) all zero. Actually, `2001:0db8:0000:....:0000` — the lower 120 bits are zero, so it's /8-aligned (and thus /108-aligned). ✓
- Risk: None — just documenting the reasoning.

### Warning 4: Test 4 (nomerge) — 100K non-adjacent /32s may overflow the 10.x.x.x range
- Test: `stress_nomerge_v4.rs` test_100k
- Issue: 100000 /32s at stride=2 means addresses 0, 2, 4, ..., 199998 relative to base `10.0.0.0`. Max offset = 199998. In the `10.0.0.0/8` space, this is `10.0.0.0` + 199998 = `10.3.13.62`. Well within the /8. ✓
- Risk: None for 100K. For larger scales, verify the address space isn't exhausted.

## Confirmed Correct

- **Test 1 (sequential /32s)**: Binary decomposition logic is correct. N=65536 → one /16. N=1048576 → one /12 (base is /12-aligned). Zero over-coverage for contiguous ranges is correct (each decomposition block is fully populated).
- **Test 2 (sequential /128s)**: Same logic applies in IPv6 space. N=65536 → one /112. N=1048576 → one /108.
- **Test 3 (full subnets)**: 256 /32s per /24 merge perfectly. Contiguous /24s further merge. N=4096 /24s = one /12 (4096 = 2^12, base is /12-aligned). Zero over-coverage is correct.
- **Test 4 (nomerge stride=2)**: Stride=2 /32s starting from even addresses means all addresses are even. Sibling of even address `2k` is `2k+1` (odd), never in the set. No /32 sibling pairs → no merges at any level → output = input count. ✓
- **Test 6a (dense /16)**: 65536 /32s in a /16 → cascading merge to one /16. ✓
- **Test 6b (alternating bit pattern)**: Even addresses in .0 subnet, odd in .1 subnet — no sibling pairs exist across or within either set. ✓
- **Test 6c (maximum redundancy)**: /8 subsumes all /16s and /24s within it. Redundancy elimination produces one /8. ✓
- **Coverage invariant**: All tests correctly assert `validate_coverage()`. The library enforces this as a safety check before returning results.
- **Default `max_prefix_len` values**: Defaults are 32 (IPv4) and 128 (IPv6), meaning no truncation occurs for /32 and /128 inputs. The plan's tests use default config, so no unexpected truncation. ✓
