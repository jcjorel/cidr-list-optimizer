---
iteration: 1
status: complete
files_count: 5
---

# Implementation Plan v1

## Summary

Fix the O(N×M) `validate_coverage` bottleneck, add a `stress` feature gate, create shared test helpers, and implement two representative stress test files (sequential-merge and no-merge patterns).

## Dependency Graph

```
1.1 Cargo.toml (stress feature)  ──┐
1.2 validate_coverage fix         ──┼──> 2.1 tests/common/mod.rs ──┬──> 2.2 stress_sequential_v4.rs
                                    │                               └──> 2.3 stress_nomerge_v4.rs
                                    │
                                    └──> (existing tests still pass)
```

## Phase 1 — Scaffold

### 1.1 `crates/cidr-optimizer/Cargo.toml`
- Action: modify
- Purpose: Add `stress` feature for gating expensive tests
- Key logic: Add `[features]` section with `stress = []`; add `ipnet` to `[dev-dependencies]` (already present as main dep, but tests need it directly)
- Dependencies: none
- Testability: `cargo check --features stress` succeeds; `cargo test` still passes

### 1.2 `crates/cidr-optimizer/src/lib.rs`
- Action: modify
- Purpose: Replace O(N×M) `validate_coverage` with O(N log M) using binary search
- Key logic:
  1. Sort output entries by network address (separately for v4/v6)
  2. For each input prefix, binary-search the sorted output for a containing prefix
  3. A prefix A contains B iff `A.network() <= B.network()` AND `A.broadcast() >= B.broadcast()`
  4. Search strategy: binary search for the rightmost output whose network ≤ input's network, then scan backwards checking containment (at most O(log M) candidates due to prefix nesting properties)
- Dependencies: none
- Testability: All existing tests pass (`cargo test`); the function signature is unchanged

## Phase 2 — Build

### 2.1 `crates/cidr-optimizer/tests/common/mod.rs`
- Action: create
- Purpose: Shared helper utilities for all stress tests
- Key logic:
  - `pub fn time_it<F: FnOnce() -> R, R>(label: &str, f: F) -> R` — wraps closure, prints elapsed to stderr
  - `pub fn generate_contiguous_v4(base: std::net::Ipv4Addr, count: u32) -> Vec<ipnet::IpNet>` — sequential /32s
  - `pub fn generate_contiguous_v6(base: std::net::Ipv6Addr, count: u64) -> Vec<ipnet::IpNet>` — sequential /128s
  - `pub fn minimal_prefix_decomposition_count(start: u32, count: u32) -> usize` — binary decomposition: iteratively subtract largest power-of-2 aligned block fitting in [start, start+count), count iterations
- Dependencies: 1.1 (needs `ipnet` in dev-deps, already satisfied)
- Testability: Create a `#[test]` in the module verifying `minimal_prefix_decomposition_count(0, 65536) == 1` and `minimal_prefix_decomposition_count(0, 10000)` returns a known value (14 — manually computed)

### 2.2 `crates/cidr-optimizer/tests/stress_sequential_v4.rs`
- Action: create
- Purpose: Stress test — contiguous /32s that merge maximally
- Key logic:
  - `test_10k_sequential_v4`: 10000 /32s from `10.0.0.0`, assert output count = `minimal_prefix_decomposition_count(0x0A000000, 10000)`
  - `test_65536_sequential_v4`: 65536 /32s, assert output = 1 entry (`10.0.0.0/16`)
  - `test_100k_sequential_v4` (ignored): 100000 /32s, assert output count matches decomposition
  - `test_1m_sequential_v4` (ignored): 1048576 /32s, assert output = 1 entry (`10.0.0.0/12`)
  - All assert `validate_coverage(input, output) == true`
  - All use `time_it` wrapper
  - 100K/1M gated: `#[cfg_attr(not(feature = "stress"), ignore)]`
- Dependencies: 1.2 (validate_coverage fix), 2.1 (helpers)
- Testability: `cargo test stress_sequential_v4` passes (10K + 65536 tests run; 100K/1M ignored)

### 2.3 `crates/cidr-optimizer/tests/stress_nomerge_v4.rs`
- Action: create
- Purpose: Stress test — non-adjacent /32s that cannot merge (worst case for trie leaf count)
- Key logic:
  - Generate N /32s at even addresses: `10.0.0.0`, `10.0.0.2`, `10.0.0.4`, ... (stride=2)
  - `test_10k_nomerge_v4`: 10000 entries, assert output count == 10000
  - `test_100k_nomerge_v4` (ignored): 100000 entries, assert output count == 100000
  - `test_1m_nomerge_v4` (ignored): 1000000 entries, assert output count == 1000000
  - All assert `validate_coverage(input, output) == true`
  - All use `time_it` wrapper
  - Generation helper: `fn generate_nomerge_v4(count: u32) -> Vec<IpNet>` (local to file)
- Dependencies: 1.2 (validate_coverage fix is CRITICAL here — without it, 100K would timeout), 2.1 (helpers)
- Testability: `cargo test stress_nomerge_v4` passes (10K runs; 100K/1M ignored)

## Phase 3 — Integrate (deferred)

### 3.1 Remaining Phase 1 test files
- `stress_sequential_v6.rs`, `stress_subnets_v4.rs`, `stress_lossy_deterministic.rs`, `stress_adversarial.rs`, `stress_differential_v4v6.rs`

### 3.2 Phase 2 test files
- `stress_mixed_and_provenance.rs`, `stress_config_constraints.rs`

## Implementer Scope

Tasks the implementer MUST complete this iteration:
- 1.1 — Add `stress` feature to Cargo.toml
- 1.2 — Fix `validate_coverage` to O(N log M)
- 2.1 — Create `tests/common/mod.rs` helper module
- 2.2 — Create `tests/stress_sequential_v4.rs`
- 2.3 — Create `tests/stress_nomerge_v4.rs`

Tasks deferred to future iterations:
- 3.1 — Test files 2, 3, 5, 6, 7
- 3.2 — Test files 8, 9

## Testing Strategy

1. After 1.1+1.2: `cargo test` — all existing tests pass (regression check)
2. After 2.1: `cargo test common` — helper self-test passes
3. After 2.2: `cargo test stress_sequential_v4` — 10K+65536 tests pass in <5s
4. After 2.3: `cargo test stress_nomerge_v4` — 10K test passes in <10s
5. Full validation: `cargo test --features stress` — all tests including 100K/1M run (may take 1-3 min)

## Edge Cases

- `validate_coverage` fix: must handle mixed v4/v6 inputs, empty output, single-entry output
- `minimal_prefix_decomposition_count`: start=0 edge, count=0 edge, non-aligned start
- Sequential generation: u32 overflow when base + count > 2^32 (use `10.0.0.0` base which is safe up to ~6M)
- No-merge generation: stride=2 means max count is ~2 billion (u32 space / 2), 1M is safe
- Feature gate: `#[cfg_attr(not(feature = "stress"), ignore)]` must be on the test function, not the module
