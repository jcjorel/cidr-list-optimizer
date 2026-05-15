# Stress Test Suite — Implementation Plan (Revised)

## Overview

Deterministic stress tests that programmatically generate large CIDR/IP inputs using mathematical patterns where the expected output is analytically predictable. No randomness. Each test is both a generator and an oracle.

## Principles

1. **No randomness** — Every input is generated from a deterministic formula
2. **Predictable output** — Expected output count/content is analytically derived from the generation pattern
3. **Coverage invariant** — Every test asserts `validate_coverage(input, output) == true`
4. **Timing** — Each test prints elapsed time via `eprintln!` for performance baselines
5. **All scales** — Every test has 10K, 100K, AND 1M entry versions. 10K runs in normal `cargo test`; 100K/1M behind `#[cfg_attr(not(feature = "stress"), ignore)]`
6. **No memory fear** — We do not constrain memory. If a test needs 10GB, so be it.

## Dependencies

None new. Only `ipnet` (already in dev-deps).

## Implementation Phases

### Phase 0 — Prerequisite: Fix `validate_coverage` to O(N log M)

**BLOCKING**: The current `validate_coverage` is O(N×M) (nested loop). For no-merge tests at 100K entries this means 10 billion comparisons — infeasible. Since `validate_coverage` is called inside `optimize_with_progress` (the CoverageLost safety check), the library itself will timeout on large no-merge inputs.

**Fix**: Replace with O(N log M) algorithm:
1. Sort output prefixes by network address
2. For each input prefix, binary-search output for the containing prefix
3. A prefix A contains prefix B if `A.network() <= B.network()` and `A.broadcast() >= B.broadcast()`

**File**: `crates/cidr-optimizer/src/lib.rs` (modify `validate_coverage`)

**Testability**: Existing tests continue to pass; new benchmark shows O(N log M) scaling.

### Phase 1 — Core Test Files (7 subagents, parallel)

### Phase 2 — Coverage Gap Additions (3 subagents, parallel)

---

## Phase 1 Test Files

### Test File 1: `tests/stress_sequential_v4.rs`

**Strategy**: Generate N contiguous IPv4 /32 addresses starting from `10.0.0.0`.

**Generation**: `10.0.0.0/32`, `10.0.0.1/32`, ..., `10.0.0.0 + N-1`

**Expected output reasoning**:
- Contiguous /32s merge into the minimal prefix decomposition of the range
- Binary decomposition: iteratively subtract the largest power-of-2 aligned block
- For N=10000: binary decomposition of [0, 10000) → specific count (compute in helper)
- For N=65536: exactly one /16
- For N=1048576 (2^20): exactly one /12 (because `10.0.0.0` is /12-aligned: lower 20 bits = 0)

**Tests**:
- `test_10k_sequential_v4` — 10000 /32s, assert output count = `minimal_prefix_decomposition(10.0.0.0, 10000)`
- `test_65536_sequential_v4` — 65536 /32s, assert output = exactly 1 entry (`10.0.0.0/16`)
- `#[ignore] test_100k_sequential_v4` — 100000 /32s, assert output count = `minimal_prefix_decomposition(10.0.0.0, 100000)`
- `#[ignore] test_1m_sequential_v4` — 1048576 /32s, assert output = exactly 1 entry (`10.0.0.0/12`)

**Assertions**:
- `validate_coverage(input, output)` is true
- Output count matches analytical expectation
- Zero over-coverage (contiguous range → lossless is perfect)
- Print elapsed time

---

### Test File 2: `tests/stress_sequential_v6.rs`

**Strategy**: Generate N contiguous IPv6 /128 addresses starting from `2001:db8::`.

**Generation**: `2001:db8::0/128`, `2001:db8::1/128`, ..., `2001:db8::N-1`

**Expected output reasoning**:
- Same binary decomposition logic as IPv4 but in 128-bit space
- For N=65536 (2^16): one /112
- For N=1048576 (2^20): one /108
- Tests path compression efficiency (deep trie with /128 leaves)

**Tests**:
- `test_10k_sequential_v6` — 10000 /128s, assert output count = binary decomposition of [0, 10000)
- `test_65536_sequential_v6` — 65536 /128s, assert output = exactly 1 entry (`2001:db8::/112`)
- `#[ignore] test_100k_sequential_v6` — 100000 /128s, assert output count = binary decomposition of [0, 100000)
- `#[ignore] test_1m_sequential_v6` — 1048576 /128s, assert output = exactly 1 entry (`2001:db8::/108`)

**Assertions**:
- Coverage invariant
- Output count matches binary decomposition of [0, N)
- Print elapsed time

---

### Test File 3: `tests/stress_subnets_v4.rs`

**Strategy**: Generate N contiguous /24 subnets, each fully populated with 256 /32s.

**Generation**: For /24 index `i` (0-based), base = `10.0.0.0 + i*256`, emit all 256 /32s within it.

**Expected output reasoning**:
- Each set of 256 /32s in a /24 → merges to one /24 via sibling cascading
- N contiguous /24s → further merge via binary decomposition of N /24-blocks
- For N=40 (10240 /32s): minimal prefix decomposition of 40 contiguous /24s
- For N=4096 (1048576 /32s): 4096 contiguous /24s = one /12

**Tests**:
- `test_10k_full_subnets_v4` — 40 /24s × 256 = 10240 inputs
- `#[ignore] test_100k_full_subnets_v4` — 400 /24s × 256 = 102400 inputs
- `#[ignore] test_1m_full_subnets_v4` — 4096 /24s × 256 = 1048576 inputs, assert output = 1 entry

**Assertions**:
- Coverage invariant
- Output count = minimal prefix decomposition of N contiguous /24s
- Zero over-coverage
- Print elapsed time

---

### Test File 4: `tests/stress_nomerge_v4.rs`

**Strategy**: Generate N non-adjacent /32s that CANNOT merge (stride=2, every other IP).

**Generation**: `10.0.0.0/32`, `10.0.0.2/32`, `10.0.0.4/32`, ... (even addresses only)

**Why no merges**: For /32s, siblings are pairs differing only in the last bit: (0,1), (2,3), (4,5), etc. Even addresses (0, 2, 4, ...) are each the LEFT sibling of their pair, but the RIGHT sibling (1, 3, 5, ...) is never present. Therefore no sibling pair is complete → no merging.

**Expected output**: Output count = input count exactly.

**Scale limit**: None. 1M non-mergeable entries will use significant memory but that's acceptable.

**Tests**:
- `test_10k_nomerge_v4` — 10000 non-adjacent /32s, assert output = 10000 entries
- `#[ignore] test_100k_nomerge_v4` — 100000 non-adjacent /32s, assert output = 100000 entries
- `#[ignore] test_1m_nomerge_v4` — 1000000 non-adjacent /32s, assert output = 1000000 entries

**Assertions**:
- Coverage invariant
- Output count == input count (no merging possible)
- Zero over-coverage
- Print elapsed time (worst case for trie: maximum leaf count)

---

### Test File 5: `tests/stress_lossy_deterministic.rs`

**Strategy**: Generate N evenly-spaced /32s across the IPv4 space, then apply lossy optimization with known targets.

**Generation**: N /32s at addresses `i * stride` where stride = `2^32 / N` (integer division, evenly distributed).

**Config**: `max_over_coverage_ratio: None` (MUST be explicit to guarantee target is reachable).

**Expected output reasoning**:
- With target=T and `max_over_coverage_ratio: None`, the optimizer MUST reach output ≤ T
- Evenly spaced /32s: each output prefix covers `ceil(N/T)` inputs minimum
- Over-coverage is bounded: total over-coverage < total address space (4 billion)

**Tests**:
- `test_10k_lossy_target_100` — 10000 spaced /32s, target=100, assert output ≤ 100
- `test_10k_lossy_target_10` — 10000 spaced /32s, target=10, assert output ≤ 10
- `#[ignore] test_100k_lossy_target_100` — 100000 spaced /32s, target=100, assert output ≤ 100
- `#[ignore] test_100k_lossy_target_10` — 100000 spaced /32s, target=10, assert output ≤ 10
- `#[ignore] test_1m_lossy_target_100` — 1000000 spaced /32s, target=100, assert output ≤ 100
- `#[ignore] test_1m_lossy_target_10` — 1000000 spaced /32s, target=10, assert output ≤ 10

**Assertions**:
- Coverage invariant (critical: lossy MUST still cover all inputs)
- Output count ≤ target
- Over-coverage bounded (sum of over_coverage < 2^32)
- Print elapsed time

---

### Test File 6: `tests/stress_adversarial.rs`

**Strategy**: Crafted worst-case patterns with predictable outputs.

#### 6a: All /32s in a single /16 (dense cascade)
- **Input**: All 65536 /32s in `10.0.0.0/16`
- **Expected**: Exactly 1 output (`10.0.0.0/16`)
- Tests maximum sibling merging cascade depth (16 levels of merging)

#### 6b: Alternating bit pattern (no merges)
- **Input**: /32s at even addresses in `10.0.0.x` + odd addresses in `10.0.1.x` (128 + 128 = 256 entries)
- **Expected**: 256 outputs (no sibling pairs exist across octets)
- Tests that the optimizer doesn't incorrectly merge non-siblings

#### 6c: Maximum redundancy (nested prefixes)
- **Input**: `10.0.0.0/8` + all 256 /16s within it + all 65536 /24s within it = 65793 entries
- **Expected**: Exactly 1 output (`10.0.0.0/8`) — all children are redundant
- Tests redundancy elimination at scale

#### 6d: Sibling pairs only (no cascading) — CORRECTED
- **Input**: 5000 pairs of siblings at NON-ADJACENT /24 positions:
  - For i=0..4999: `10.{(i*2)/256}.{(i*2)%256}.0/25` + `10.{(i*2)/256}.{(i*2)%256}.128/25`
  - This produces /24s at indices 0, 2, 4, 6, ... (stride=2) which are NOT siblings at the /24 level
- **Expected**: Exactly 5000 /24s (each /25 pair merges to its /24, but /24s don't cascade)
- Tests pure sibling merging without cascading

#### 6e: Aggressive lossy collapse to single entry — CORRECTED
- **Input**: First 10000 contiguous /32s within `10.0.0.0/18` (IPs 10.0.0.0 – 10.0.39.15)
- **Config**: `ipv4_target: Some(1)`, `max_over_coverage_ratio: None`
- **Expected**: Exactly 1 output = `10.0.0.0/18` (because inputs span both /19 halves: 10000 > 8192, so LCA is /18)
- Tests aggressive lossy collapse; asserts exact prefix

#### 6f: N=1 edge cases (NEW)
- **Input a**: Single `10.0.0.1/32`, no target → output = 1 entry
- **Input b**: Single `10.0.0.1/32`, target=1 → output = 1 entry
- **Input c**: Single `2001:db8::1/128`, provenance=true → output = 1 entry with source_indices=[0]
- Tests degenerate single-entry paths through trie builder, optimizer, provenance

**Assertions per sub-test**:
- Coverage invariant
- Exact output count (and exact prefix where specified)
- Print elapsed time

---

### Test File 7: `tests/stress_differential_v4v6.rs`

**Strategy**: Generate deterministic inputs and compare lossless output against `ipnet::aggregate()` as reference.

**Comparison method**: Both outputs sorted by `(network_addr, prefix_len)` ascending, then direct `Vec<IpNet>` equality.

**Generation patterns**:
- 10K contiguous /32s starting at `10.0.0.0`
- 10K contiguous /24s (as IpNet, not expanded to /32s)
- 10K mixed: 3333 /16s + 3333 /24s + 3334 /32s with overlaps (nested within `10.0.0.0/8`)
- 10K IPv6 /128s starting at `2001:db8::`
- 100K contiguous /32s (ignored)

**Expected output reasoning**:
- `ipnet::aggregate()` is the reference implementation for lossless aggregation
- Our lossless output (no target set) MUST produce the identical prefix set
- Both implementations handle: redundancy elimination, sibling merging, non-canonical inputs

**Tests**:
- `test_10k_differential_contiguous_v4` — compare outputs
- `test_10k_differential_contiguous_v24` — compare outputs
- `test_10k_differential_mixed` — compare outputs
- `test_10k_differential_v6` — compare outputs
- `#[ignore] test_100k_differential_v4` — compare outputs at scale
- `#[ignore] test_100k_differential_v6` — compare outputs at scale
- `#[ignore] test_1m_differential_v4` — compare outputs at 1M scale
- `#[ignore] test_1m_differential_v6` — compare outputs at 1M scale

**Assertions**:
- Output prefix sets are identical (sorted Vec equality)
- Coverage invariant
- Print elapsed time for both implementations (relative performance)

---

## Phase 2 — Coverage Gap Additions

### Test File 8: `tests/stress_mixed_and_provenance.rs`

#### 8a: Mixed IPv4+IPv6 with provenance
- **Input 10K**: 5000 sequential IPv4 /32s (`10.0.0.0`–`10.0.19.135`) + 5000 sequential IPv6 /128s (`2001:db8::0`–`2001:db8::1387`)
- **Input 100K**: 50000 IPv4 /32s + 50000 IPv6 /128s
- **Input 1M**: 500000 IPv4 /32s + 500000 IPv6 /128s
- **Config**: `provenance: true`
- **Expected**: Provenance `source_indices` for IPv4 entries reference indices 0–(N/2-1); IPv6 entries reference indices (N/2)–(N-1)
- Tests partition_with_indices index mapping and v4_idx/v6_idx interleaving in build_result

#### 8b: Provenance completeness under lossy optimization
- **Input 10K**: 10000 sequential /32s starting at `10.0.0.0`
- **Input 100K**: 100000 sequential /32s
- **Input 1M**: 1000000 sequential /32s
- **Config**: `ipv4_target: Some(100)`, `provenance: true`, `max_over_coverage_ratio: None`
- **Expected**: Union of all `source_indices` across output entries == `{0, 1, ..., N-1}` (every input appears in exactly one output's provenance)
- Tests provenance binary search boundaries under lossy collapse

#### 8c: Duplicate inputs
- **Input 10K**: `10.0.0.0/24` repeated 5000 times + `192.168.0.0/24` repeated 5000 times = 10000 entries
- **Input 100K**: each repeated 50000 times = 100000 entries
- **Input 1M**: each repeated 500000 times = 1000000 entries
- **Config**: `provenance: true`
- **Expected**: Output = 2 entries; each entry's `source_indices` has N/2 elements
- Tests redundancy elimination with duplicates and provenance accumulation

### Test File 9: `tests/stress_config_constraints.rs`

#### 9a: `max_over_coverage_ratio` cap prevents reaching target
- **Input 10K**: 10000 /32s at stride=65536 (widely spaced)
- **Input 100K**: 100000 /32s at stride=4295 (widely spaced)
- **Input 1M**: 1000000 /32s at stride=4295 (wrapping within address space)
- **Config**: `ipv4_target: Some(10)`, `max_over_coverage_ratio: Some(0.01)`
- **Expected**: Output count > 10 (ratio cap stops optimization early); ratio is respected
- Tests 160-bit widening multiplication overflow logic in `exceeds_ratio`

#### 9b: `max_prefix_len_v4` enforcement at scale
- **Input 10K**: 10000 contiguous /32s within `10.0.0.0/16`
- **Input 100K**: 100000 contiguous /32s
- **Input 1M**: 1000000 contiguous /32s
- **Config**: `max_prefix_len_v4: 24`
- **Expected**: Merging stops at /24 boundary. Output = number of /24s touched by the input range (ceil(N/256))
- Tests that max_prefix_len correctly halts cascading merges at scale

#### 9c: Cancellation during lossy phase
- **Input 10K**: 10000 non-adjacent /32s (stride=2)
- **Input 100K**: 100000 non-adjacent /32s (stride=2)
- **Input 1M**: 1000000 non-adjacent /32s (stride=2)
- **Config**: `ipv4_target: Some(10)`, progress callback cancels on `Phase::Lossy`
- **Expected**: `Err(OptimizeError::Cancelled)`
- Tests that cancellation works during long-running lossy optimization at all scales

---

## Helper Module: `tests/common/mod.rs`

```rust
// Shared utilities for stress tests

/// Wraps a closure, prints elapsed time to stderr, returns the result.
fn time_it<F: FnOnce() -> R, R>(label: &str, f: F) -> R

/// Generates N sequential IPv4 /32s starting from base.
fn generate_contiguous_v4(base: Ipv4Addr, count: u32) -> Vec<IpNet>

/// Generates N sequential IPv6 /128s starting from base.
fn generate_contiguous_v6(base: Ipv6Addr, count: u64) -> Vec<IpNet>

/// Computes expected output count for a contiguous range of /32s.
/// Algorithm: binary decomposition — iteratively subtract the largest
/// power-of-2 aligned block that fits within [start, start+count).
/// A block of size 2^k is aligned if start % 2^k == 0.
fn minimal_prefix_decomposition_count(start: u32, count: u32) -> usize
```

---

## Execution Summary

| Normal `cargo test` | `cargo test --features stress` |
|---------------------|-------------------------------|
| All 10K tests (~25 tests) | ALL tests: 10K + 100K + 1M (~75 tests) |
| Should complete in < 30s | May take 2-10 minutes depending on hardware |

Every test file has 10K, 100K, AND 1M variants. No exceptions.

## Scale Gating

Use `#[cfg_attr(not(feature = "stress"), ignore)]` for 100K/1M tests. Add to `crates/cidr-optimizer/Cargo.toml`:

```toml
[features]
stress = []
```

This allows CI to run `cargo test --features stress` in a nightly/weekly job.

## Implementation Order

1. **Phase 0**: Fix `validate_coverage` to O(N log M) — single subagent
2. **Phase 1**: Test Files 1–7 — 7 parallel subagents
3. **Phase 2**: Test Files 8–9 — 2 parallel subagents

Total: 10 subagent invocations across 3 sequential phases.
