---
iteration: 2
status: complete
files_count: 4
---

# Implementation Plan v2

## Summary

Implement 4 additional stress test files: IPv6 sequential merging, full subnet merging, lossy optimization with deterministic targets, and adversarial worst-case patterns. All use the existing `tests/common/mod.rs` helpers.

## Changes from v1

Iteration 1 completed successfully (PASS). This iteration adds 4 more test files from the spec (Test Files 2, 3, 5, 6).

## Dependency Graph

All 4 test files depend on `tests/common/mod.rs` (already exists). No new dependencies needed.

## Phase 2 — Build

### 2.1 `crates/cidr-optimizer/tests/stress_sequential_v6.rs`
- Action: create
- Purpose: Stress test — contiguous IPv6 /128s that merge maximally (tests deep trie path compression)
- Key logic:
  - Use `common::generate_contiguous_v6(Ipv6Addr::from_str("2001:db8::").unwrap(), N)`
  - Binary decomposition count for IPv6 is identical logic to IPv4 (same `minimal_prefix_decomposition_count` but on u128). Since the helper only handles u32, implement a local `minimal_prefix_decomposition_count_v6(start: u128, count: u64) -> usize` with same algorithm adapted to u128.
  - `test_10k_sequential_v6`: 10000 /128s, assert output count = decomposition of [0x20010db8_00000000_00000000_00000000, 10000)
  - `test_65536_sequential_v6`: 65536 /128s, assert output = 1 entry (`2001:db8::/112`)
  - `test_100k_sequential_v6` (ignored): 100000 /128s, assert output count = decomposition
  - `test_1m_sequential_v6` (ignored): 1048576 /128s, assert output = 1 entry (`2001:db8::/108`)
  - All assert `validate_coverage` + use `time_it`
  - Note: `2001:db8::` = `0x20010db800000000_0000000000000000`. Lower 112 bits are 0, so 65536=2^16 aligns to /112; 2^20 aligns to /108.
- Dependencies: common/mod.rs
- Testability: `cargo test stress_sequential_v6`

### 2.2 `crates/cidr-optimizer/tests/stress_subnets_v4.rs`
- Action: create
- Purpose: Stress test — full /24 subnets (256 /32s each) that cascade-merge into larger blocks
- Key logic:
  - Local generator: for N /24s, base=`10.0.0.0`, emit all 256 /32s in each /24 (total = N*256 inputs)
  - Expected: each 256 /32s → one /24, then N contiguous /24s merge via binary decomposition
  - Use `minimal_prefix_decomposition_count(0x0A000000 >> 8, N)` but adapted: the /24 blocks start at /24 index `(0x0A000000 >> 8)` = `0x0A0000`. Need a local helper that computes decomposition of N contiguous /24-aligned blocks. Simplification: since `10.0.0.0` has lower 8 bits = 0, N contiguous /24s starting there is equivalent to `minimal_prefix_decomposition_count(0x0A000000, N * 256)` (same as N*256 contiguous /32s).
  - `test_10k_full_subnets_v4`: 40 /24s × 256 = 10240 inputs, assert output count = `minimal_prefix_decomposition_count(0x0A000000, 40 * 256)`
  - `test_100k_full_subnets_v4` (ignored): 400 /24s × 256 = 102400 inputs
  - `test_1m_full_subnets_v4` (ignored): 4096 /24s × 256 = 1048576 inputs, assert output = 1 entry (`10.0.0.0/12`)
  - All assert `validate_coverage` + use `time_it`
- Dependencies: common/mod.rs
- Testability: `cargo test stress_subnets_v4`

### 2.3 `crates/cidr-optimizer/tests/stress_lossy_deterministic.rs`
- Action: create
- Purpose: Stress test — evenly-spaced /32s with lossy target, verifying target is reached
- Key logic:
  - Local generator: N /32s at addresses `i * stride` where `stride = 2^32 / N` (integer division). Use `u32` arithmetic: `Ipv4Addr::from((i as u64 * stride as u64) as u32)` to handle large N.
  - Config: `OptimizerConfig { ipv4_target: Some(target), max_over_coverage_ratio: None, ..Default::default() }`
  - `max_over_coverage_ratio: None` is already the default, but set explicitly for clarity
  - `test_10k_lossy_target_100`: 10000 /32s, target=100, assert output.len() ≤ 100
  - `test_10k_lossy_target_10`: 10000 /32s, target=10, assert output.len() ≤ 10
  - `test_100k_lossy_target_100` (ignored): 100000 /32s, target=100
  - `test_100k_lossy_target_10` (ignored): 100000 /32s, target=10
  - `test_1m_lossy_target_100` (ignored): 1000000 /32s, target=100
  - `test_1m_lossy_target_10` (ignored): 1000000 /32s, target=10
  - All assert: `validate_coverage`, output.len() ≤ target, `time_it`
- Dependencies: common/mod.rs
- Testability: `cargo test stress_lossy`

### 2.4 `crates/cidr-optimizer/tests/stress_adversarial.rs`
- Action: create
- Purpose: Adversarial worst-case patterns with analytically predictable outputs
- Key logic — 6 sub-tests (all run in normal `cargo test`, no ignore needed since inputs are small):
  - **6a** `test_dense_cascade`: All 65536 /32s in `10.0.0.0/16` → assert output = 1 entry (`10.0.0.0/16`)
  - **6b** `test_alternating_no_merge`: 128 even /32s in `10.0.0.x` + 128 odd /32s in `10.0.1.x` = 256 entries → assert output = 256 (no sibling pairs across different /24 halves). Generation: `10.0.0.{0,2,4,...,254}/32` + `10.0.1.{1,3,5,...,255}/32`
  - **6c** `test_maximum_redundancy`: `10.0.0.0/8` + all 256 /16s in it + all 65536 /24s in it = 65793 entries → assert output = 1 (`10.0.0.0/8`)
  - **6d** `test_sibling_pairs_no_cascade`: 5000 pairs of /25 siblings at non-adjacent /24 positions (stride=2 on /24 index). For i=0..4999: emit `10.{(i*2)/256}.{(i*2)%256}.0/25` and `10.{(i*2)/256}.{(i*2)%256}.128/25`. → assert output = 5000 /24s (pairs merge to /24, but /24s at stride=2 don't cascade)
  - **6e** `test_aggressive_lossy_collapse`: 10000 contiguous /32s in `10.0.0.0/18` (IPs 10.0.0.0–10.0.39.15), config `ipv4_target: Some(1)`, `max_over_coverage_ratio: None` → assert output = 1 entry. The LCA is /18 because 10000 > 8192 (spans both /19 halves).
  - **6f** `test_single_entry_edge_cases`: (a) single `10.0.0.1/32` no target → 1 output; (b) single `10.0.0.1/32` target=1 → 1 output; (c) single `2001:db8::1/128` provenance=true → 1 output with source_indices=[0]
  - All assert `validate_coverage` + use `time_it`
- Dependencies: common/mod.rs
- Testability: `cargo test stress_adversarial`

## Phase 3 — Integrate (deferred)

Remaining test files deferred to iteration 3:
- Test File 7: `stress_differential_v4v6.rs`
- Test File 8: `stress_mixed_and_provenance.rs`
- Test File 9: `stress_config_constraints.rs`

## Implementer Scope

Tasks: 2.1, 2.2, 2.3, 2.4

## Testing Strategy

1. After each file: `cargo test <test_file_name>` — 10K tests pass
2. Full: `cargo test` — all non-ignored tests pass (including iteration 1 tests)
3. Stress: `cargo test --features stress` — all tests including 100K/1M run

## Edge Cases

- IPv6 decomposition: u128 arithmetic must not overflow; `2001:db8::` lower bits are 0 so alignment is clean
- Lossy stride calculation: for N=1M, stride = 4294 (integer division of 2^32/1M); last address may not reach end of space — acceptable
- Adversarial 6c: 65793 entries with nested redundancy — optimizer must eliminate all children of /8
- Adversarial 6e: LCA assertion depends on optimizer choosing /18 not something larger; with target=1 and no ratio cap, /18 is the minimal covering prefix
- Adversarial 6d: stride=2 on /24 index means /24s at positions 0,2,4,... — these are left siblings whose right siblings (1,3,5,...) are absent, so no /23 merging occurs
