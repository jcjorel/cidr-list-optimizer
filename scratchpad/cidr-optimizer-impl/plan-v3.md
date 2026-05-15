---
iteration: 3
status: complete
files_count: 4
---

# Implementation Plan v3

## Summary
Final iteration: fix over-coverage reporting by tracking coverage in trie-extracted leaves, implement `--validate` flag, add provenance support to JSON output, and suppress the clippy warning.

## Changes from v2
- v2 delivered the full lossy optimization pipeline (trie + optimizer + lib API + CLI wiring). All compiles and tests pass.
- v3 addresses the two feedback issues (over-coverage misreport, clippy warning) and completes the remaining Phase 4 features: provenance extraction from trie leaves, `--validate` implementation, and JSON/AWS output with provenance data.
- Scope is 5 tasks focused on correctness fixes and output completeness.

## Dependency Graph
```
lossless.rs (add coverage field to ProvenancePrefix) → trie.rs (store coverage in extracted leaves) → lib.rs (use coverage directly, fix over-coverage) → main.rs (--validate, JSON provenance output)
provenance.rs (binary-search mapping) → lib.rs (wire provenance when enabled)
```
- `lossless.rs` change is independent (adds coverage field to `ProvenancePrefix` struct)
- `trie.rs` depends on lossless.rs (uses the new coverage field)
- `provenance.rs` is a new module, depends only on existing types
- `lib.rs` depends on both trie.rs fix and provenance.rs
- `main.rs` depends on lib.rs changes

## Phase 1 — Scaffold

### 1.1 `crates/cidr-optimizer/src/lossless.rs`
- Action: modify
- Purpose: Add `coverage` field to `ProvenancePrefix` struct, set it to capacity for lossless entries
- Key logic:
  - Add `pub coverage: u128` to `ProvenancePrefix<N>`
  - In `lossless_aggregate_v4`: set `coverage` = capacity of the prefix (`1 << (32 - prefix_len)`) for each output entry
  - In `lossless_aggregate_v6`: set `coverage` = capacity of the prefix (`1 << (128 - prefix_len)`, saturating for /0)
  - This ensures lossless entries report zero over-coverage (coverage == capacity)
- Dependencies: none
- Testability: Unit test — `lossless_aggregate_v4` on `[10.0.0.0/24]` produces entry with `coverage == 256`

### 1.2 `crates/cidr-optimizer/src/trie.rs`
- Action: modify
- Purpose: Include the node's `coverage` value in extracted `ProvenancePrefix` so over-coverage can be computed correctly without provenance
- Key logic:
  - In `extract_leaves_v4/v6`, set `coverage` to `self.arena[idx as usize].coverage`
- Dependencies: 1.1 (requires the `coverage` field on `ProvenancePrefix`)
- Testability: Unit test — build trie from `[10.0.0.0/24, 10.0.1.0/24]`, collapse to /23, extract leaves, verify `coverage == 512` (256+256)

### 1.3 `crates/cidr-optimizer/src/provenance.rs`
- Action: create
- Purpose: Binary-search provenance mapping from output prefixes back to original input indices
- Key logic:
  - `pub fn compute_provenance_v4(output: &[Ipv4Net], sorted_input: &[(usize, Ipv4Net)]) -> Vec<Vec<usize>>` — for each output prefix, binary search to find all input prefixes it contains
  - `pub fn compute_provenance_v6(output: &[Ipv6Net], sorted_input: &[(usize, Ipv6Net)]) -> Vec<Vec<usize>>` — same for IPv6
  - Use `partition_point` to find the range of candidates, then filter by `output.contains(input_prefix)`
  - Input must be sorted by network address (already done by parser)
- Dependencies: none
- Testability: Unit test — `compute_provenance_v4` with output `[10.0.0.0/23]` and input `[(0, 10.0.0.0/24), (1, 10.0.1.0/24)]` returns `[vec![0, 1]]`

## Phase 2 — Build

### 2.1 `crates/cidr-optimizer/src/lib.rs`
- Action: modify
- Purpose: Fix over-coverage calculation using trie coverage, wire provenance module, implement `validate_coverage`, suppress clippy warning
- Key logic:
  - Add `pub mod provenance;`
  - Remove the `CoverageHelper` trait entirely — replace with direct use of `ProvenancePrefix.coverage` field
  - In `build_result`: compute over-coverage as `capacity - entry.coverage` (no longer depends on `source_indices`)
  - When `config.provenance == true`: after building output entries, call `provenance::compute_provenance_v4/v6` to populate `source_indices`
  - Add `pub fn validate_coverage(input: &[IpNet], output: &[AggregatedEntry]) -> bool` — checks every input prefix is contained by at least one output prefix
  - Add `#[allow(clippy::type_complexity)]` on `partition_with_indices` return type (or extract type alias)
  - Store sorted input copies for provenance lookup when `config.provenance == true`
- Dependencies: 1.1, 1.2, 1.3
- Testability: Test — optimize `[10.0.0.0/24, 10.0.1.0/24, 10.0.4.0/24]` with target=2, verify `total_ipv4_over_coverage` is correct (not full capacity); test `validate_coverage` returns true for valid output, false when an input is missing

### 2.2 `crates/cidr-optimizer-cli/src/main.rs`
- Action: modify
- Purpose: Implement `--validate` flag, enhance JSON output with provenance/over-coverage, use serde_json for proper JSON serialization
- Key logic:
  - `--validate`: after optimization, call `validate_coverage()`. If fails, print error to stderr and exit with code 1. If passes, print "Validation passed: all inputs covered" to stderr.
  - JSON format: use `serde_json` to produce spec-compliant output with `prefix`, `source_count`, `sources` (when provenance enabled), `over_coverage`, and `stats` object
  - AWS format: use `serde_json` for proper escaping
  - Pass original `prefixes` slice to `validate_coverage` (requires storing parsed input before optimization — adjust to parse first, then optimize from slice)
- Dependencies: 2.1
- Testability: `echo "10.0.0.0/24\n10.0.1.0/24" | cargo run -p cidr-optimizer-cli -- --validate` prints validation passed; `--format json --provenance` includes source info; `--format json` includes over_coverage and stats

## Phase 3 — Integrate
No deferred work — this is the final iteration.

## Implementer Scope

Tasks the implementer MUST complete this iteration:
- 1.1, 1.2, 1.3, 2.1, 2.2

Tasks deferred to future iterations:
- None (final iteration). Testing (property tests, differential tests, fuzz targets) and benchmarks are out of scope but documented in `scratchpad/implementation.md` Phase 5.

Rationale: 5 tasks covering the remaining correctness fix and feature completeness. Tasks 1.1-1.2 are small struct changes. Task 1.3 is a new small module. Tasks 2.1-2.2 are medium modifications to existing files. Total scope is manageable as the algorithmic complexity was handled in v2.

## Testing Strategy
- `cargo test --workspace` must pass with zero failures
- Verify over-coverage is correct: optimize `[10.0.0.0/24, 10.0.1.0/24, 10.0.4.0/24]` with target=2 → over-coverage should be 512 (the /23 covering 10.0.2.0/24 and 10.0.3.0/24 that weren't in input... actually depends on which merge the optimizer picks). Key: over-coverage must NOT equal full capacity of output prefixes.
- Verify `--validate` passes for all valid optimizations
- Verify JSON output is valid JSON with correct structure
- Verify provenance: with `--provenance`, each output entry lists its contributing inputs

## Edge Cases
- Trie-extracted leaves with zero original sources → coverage field from trie node gives correct over-coverage
- Lossless-only entries → coverage == capacity → over-coverage == 0
- Empty provenance (provenance disabled) → `source_indices: None` in output
- `--validate` with lossy optimization → must still pass (all inputs covered)
- IPv6 /0 root collapse → coverage = u128::MAX, over-coverage = 0 (or near-zero)
- JSON output with no IPv6 entries → empty array, not omitted
