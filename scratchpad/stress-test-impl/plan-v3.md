---
iteration: 3
status: complete
files_count: 3
---

# Implementation Plan v3

## Summary
Final iteration: implement remaining 3 test files (7, 8, 9) to complete the stress test suite.

## Changes from v2
Iterations 1-2 completed (both PASS). This iteration adds the final 3 test files.

## Dependency Graph
All 3 test files depend on `tests/common/mod.rs` (already exists). Test File 7 uses `ipnet::Ipv4Net::aggregate()` and `ipnet::Ipv6Net::aggregate()` as reference oracle. No new crate dependencies needed (`ipnet = "2"` already in dev-dependencies).

## Phase 2 — Build

### 2.1 `crates/cidr-optimizer/tests/stress_differential_v4v6.rs`

**Purpose**: Differential testing — compare our lossless output against `ipnet`'s `aggregate()`.

**Imports**: `mod common;`, `cidr_optimizer::{optimize, validate_coverage, OptimizerConfig}`, `ipnet::{IpNet, Ipv4Net, Ipv6Net}`, `common::{time_it, generate_contiguous_v4, generate_contiguous_v6}`.

**Reference oracle**: `Ipv4Net::aggregate(&nets)` returns `Vec<Ipv4Net>` (sorted). `Ipv6Net::aggregate(&nets)` returns `Vec<Ipv6Net>` (sorted).

**Comparison method**: Convert our output `result.entries` to sorted `Vec<IpNet>` (by network addr then prefix_len). Convert ipnet's output to sorted `Vec<IpNet>`. Assert equality.

**Helper function** (local to file): `fn our_sorted_output(result: &[AggregatedEntry]) -> Vec<IpNet>` — extracts prefixes, sorts by `(network, prefix_len)`.

**Generation patterns & tests**:
1. `test_10k_differential_contiguous_v4` — 10K contiguous /32s from `10.0.0.0`; config: default (lossless)
2. `test_10k_differential_contiguous_v24` — 10K contiguous /24s as `IpNet` (not expanded); base `10.0.0.0/24` to `10.0.39.15/24` (i.e., `Ipv4Net::new(Ipv4Addr::from(0x0A000000 + i*256), 24)` for i in 0..10000)
3. `test_10k_differential_mixed` — 3333 /16s + 3333 /24s + 3334 /32s all within `10.0.0.0/8` (overlapping: /16s at `10.{i}.0.0/16`, /24s at `10.0.{i}.0/24`, /32s at `10.0.0.{i}/32`)
4. `test_10k_differential_v6` — 10K contiguous /128s from `2001:db8::`
5. `#[ignore] test_100k_differential_v4` — 100K contiguous /32s from `10.0.0.0`
6. `#[ignore] test_100k_differential_v6` — 100K contiguous /128s from `2001:db8::`
7. `#[ignore] test_1m_differential_v4` — 1M contiguous /32s from `10.0.0.0`
8. `#[ignore] test_1m_differential_v6` — 1M contiguous /128s from `2001:db8::`

**Assertions per test**: `validate_coverage`, sorted output equality with ipnet's aggregate, print elapsed time for both implementations.

**Note on ipnet API**: For IPv4, extract `Ipv4Net` from inputs, call `Ipv4Net::aggregate(&v4_nets)`. For IPv6, extract `Ipv6Net`, call `Ipv6Net::aggregate(&v6_nets)`. For mixed test, split into v4/v6, aggregate each separately, combine and sort.

---

### 2.2 `crates/cidr-optimizer/tests/stress_mixed_and_provenance.rs`

**Purpose**: Test mixed IPv4+IPv6 with provenance tracking, provenance completeness under lossy, and duplicate handling.

**Imports**: `mod common;`, `cidr_optimizer::{optimize, validate_coverage, OptimizerConfig}`, `ipnet::IpNet`, `std::collections::HashSet`, `common::{time_it, generate_contiguous_v4, generate_contiguous_v6}`.

**Provenance API**: `result.entries[i].source_indices` is `Option<Vec<usize>>`. When `config.provenance = true`, each entry has `Some(vec![...])` with original input indices.

**Sub-tests**:

#### 8a: Mixed IPv4+IPv6 with provenance
- Generate N/2 IPv4 /32s (indices 0..N/2-1) + N/2 IPv6 /128s (indices N/2..N-1)
- Config: `provenance: true`, default otherwise
- Assert: all IPv4 output entries have `source_indices` ⊆ {0..N/2-1}; all IPv6 entries have `source_indices` ⊆ {N/2..N-1}
- Tests: `test_10k_mixed_provenance`, `#[ignore] test_100k_mixed_provenance`, `#[ignore] test_1m_mixed_provenance`

#### 8b: Provenance completeness under lossy
- Generate N sequential /32s from `10.0.0.0`
- Config: `ipv4_target: Some(100)`, `provenance: true`, `max_over_coverage_ratio: None`
- Assert: union of all `source_indices` across entries == {0..N-1} (every input accounted for)
- Tests: `test_10k_provenance_completeness`, `#[ignore] test_100k_provenance_completeness`, `#[ignore] test_1m_provenance_completeness`

#### 8c: Duplicate inputs
- Input: `10.0.0.0/24` repeated N/2 times + `192.168.0.0/24` repeated N/2 times
- Config: `provenance: true`
- Assert: output = 2 entries; each entry's `source_indices.unwrap().len()` == N/2
- Tests: `test_10k_duplicates_provenance`, `#[ignore] test_100k_duplicates_provenance`, `#[ignore] test_1m_duplicates_provenance`

**All tests assert**: `validate_coverage`, print elapsed time.

---

### 2.3 `crates/cidr-optimizer/tests/stress_config_constraints.rs`

**Purpose**: Test `max_over_coverage_ratio`, `max_prefix_len_v4`, and cancellation.

**Imports**: `mod common;`, `std::ops::ControlFlow`, `cidr_optimizer::{optimize, optimize_with_progress, validate_coverage, OptimizerConfig, OptimizeError, Phase, AddressFamily}`, `ipnet::IpNet`, `std::net::Ipv4Addr`, `common::time_it`.

**Sub-tests**:

#### 9a: `max_over_coverage_ratio` cap prevents reaching target
- Generate N /32s at stride=65536 (for 10K) or stride=4295 (for 100K/1M): `Ipv4Addr::from((i as u64 * stride as u64 % (1u64<<32)) as u32)`
- Config: `ipv4_target: Some(10)`, `max_over_coverage_ratio: Some(0.01)`
- Assert: output count > 10 (ratio cap stopped early); verify ratio: sum of `over_coverage` / total_address_space ≤ 0.01
- Tests: `test_10k_ratio_cap`, `#[ignore] test_100k_ratio_cap`, `#[ignore] test_1m_ratio_cap`

#### 9b: `max_prefix_len_v4` enforcement
- Generate N contiguous /32s from `10.0.0.0`
- Config: `max_prefix_len_v4: 24` (merging stops at /24)
- Assert: output count == ceil(N / 256); all output prefix_len >= 24
- Tests: `test_10k_max_prefix_len`, `#[ignore] test_100k_max_prefix_len`, `#[ignore] test_1m_max_prefix_len`

#### 9c: Cancellation during lossy phase
- Generate N non-adjacent /32s (stride=2, even addresses from `10.0.0.0`)
- Config: `ipv4_target: Some(10)`, `max_over_coverage_ratio: None`
- Use `optimize_with_progress` with callback: `|phase| match phase { Phase::Lossy { .. } => ControlFlow::Break(()), _ => ControlFlow::Continue(()) }`
- Assert: result == `Err(OptimizeError::Cancelled)`
- Tests: `test_10k_cancellation`, `#[ignore] test_100k_cancellation`, `#[ignore] test_1m_cancellation`

**All non-cancelled tests assert**: `validate_coverage`, print elapsed time.

---

## Implementer Scope
Tasks: 2.1, 2.2, 2.3

## Testing Strategy
```bash
cargo test --manifest-path crates/cidr-optimizer/Cargo.toml
cargo test --manifest-path crates/cidr-optimizer/Cargo.toml --features stress
```
All 10K tests must pass in normal mode. All tests (including 100K/1M) must pass with `--features stress`.
