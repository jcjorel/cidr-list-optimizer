---
iteration: 2
status: complete
files_count: 4
---

# Implementation Plan v2

## Summary
Implement the path-compressed binary trie (`trie.rs`) and greedy optimizer (`optimizer.rs`), then wire them into the library and CLI to enable lossy `--ipv4-target`/`--ipv6-target` optimization. Also fix the two minor CLI issues from feedback.

## Changes from v1
- v1 delivered foundation + lossless aggregation (Phase 1-2 of the impl plan). All compiles and tests pass.
- v2 implements Phase 3 (trie + optimizer) which was deferred in v1 as task 3.1.
- Additionally fixes two minor CLI bugs from feedback: incorrect stats IPv4 count calculation, and `--validate` flag being silently ignored.
- Scope is 5 tasks (2 scaffold, 2 build, 1 CLI fix) — focused on the core lossy optimization path.

## Dependency Graph
```
trie.rs ← optimizer.rs ← lib.rs (expand with optimize API) ← main.rs (wire --target flags)
```
- `trie.rs` depends on existing `types.rs`, `error.rs`
- `optimizer.rs` depends on `trie.rs`
- `lib.rs` expansion depends on `optimizer.rs` + existing `lossless.rs`
- `main.rs` fixes + lossy wiring depend on `lib.rs` expansion

## Phase 1 — Scaffold

### 1.1 `crates/cidr-optimizer/src/trie.rs`
- Action: create
- Purpose: Arena-allocated path-compressed binary trie for lossy optimization
- Key logic:
  - `TrieNode` struct (80 bytes, `#[repr(C)]`, compile-time size assert): `skip_bits: u128`, `coverage: u128`, `collapsed_cost_sum: u128`, `children: [u32; 2]`, `parent: u32`, `leaf_count: u32`, `generation: u32`, `depth: u8`, `is_leaf: bool`, `skip_len: u8`, `_pad: [u8; 9]` (pad to 80)
  - `BinaryTrie` struct with `arena: Vec<TrieNode>`, `root: u32`, `addr_bits: u8`
  - `alloc_node()` with `u32::MAX` overflow guard returning `OptimizeError::ArenaOverflow`
  - `capacity(node_idx)` — returns `u128::MAX` for root (depth=0, skip_len=0), else `1u128 << (addr_bits - prefix_len)` with saturation
  - `collapse_cost(node_idx)` — `capacity.saturating_sub(coverage)`
  - `build_from_v4(lossless: &[ProvenancePrefix<Ipv4Net>])` — insert each prefix, split compressed nodes on mismatch, compute coverage bottom-up
  - `build_from_v6(lossless: &[ProvenancePrefix<Ipv6Net>])` — same for IPv6
  - `collapse(node_idx)` — set `is_leaf=true`, `leaf_count=1`, `children=[INVALID, INVALID]`
  - `invalidate_subtree(node_idx)` — increment generation of all descendants
  - `update_leaf_count(node_idx)` — recompute from children
  - `extract_leaves_v4() -> Vec<ProvenancePrefix<Ipv4Net>>` / `extract_leaves_v6()`
  - `total_leaf_count() -> usize`
  - Path compression: single-child chains compressed with `skip_len > 0`, `skip_bits` stores compressed bit pattern
  - Formal invariants enforced via `debug_assert!` (INV-1 through INV-6 from spec)
- Dependencies: `error.rs` (for `OptimizeError::ArenaOverflow`), `lossless.rs` (for `ProvenancePrefix`)
- Testability: Unit tests — build trie from known prefixes, verify `total_leaf_count()`, verify `capacity()` for root and leaves, verify path compression reduces node count for /128 entries, verify `collapse()` + `extract_leaves()` round-trip, compile-time `size_of::<TrieNode>() == 80` assert

### 1.2 `crates/cidr-optimizer/src/optimizer.rs`
- Action: create
- Purpose: Greedy collapse algorithm with BinaryHeap and cost-efficiency key
- Key logic:
  - `EfficiencyKey { cost: u128, savings: u32 }` with `Ord` impl using widening 160-bit multiplication (`widening_mul_u128_u32`)
  - `HeapEntry = Reverse<(EfficiencyKey, u32, u32)>` — (efficiency, node_idx, generation)
  - `optimize_trie(trie: &mut BinaryTrie, target: usize, max_ratio: Option<f64>, input_covered_ips: u128) -> Result<(), OptimizeError>`
  - Main loop: pop min-efficiency candidate, staleness check via generation, compute `net_new_over = cost - collapsed_cost_sum`, ratio check via `exceeds_ratio()`, update `current_over_coverage`, invalidate subtree, collapse, update ancestors (leaf_count, collapsed_cost_sum, generation bump, push new heap entries)
  - `exceeds_ratio(over: u128, covered: u128, max_ratio: f64) -> bool` — f64 fast path for values ≤ u64::MAX, integer-scaled check with `SCALE=1_000_000` for large IPv6 values, symmetric overflow handling
  - `widening_mul_u128_u32(a: u128, b: u32) -> (u32, u128)` — exact 160-bit product
  - Heap compaction when stale entries > 4× remaining count
- Dependencies: 1.1 (`trie.rs`)
- Testability: Unit tests — optimize a small trie (4 leaves) to target=2, verify correct leaves remain; verify `EfficiencyKey` ordering (cost=6/savings=3 < cost=3/savings=1); verify `exceeds_ratio` for edge cases (overflow, zero covered); verify deterministic output for tied efficiencies

## Phase 2 — Build

### 2.1 `crates/cidr-optimizer/src/lib.rs`
- Action: modify
- Purpose: Add `mod trie; mod optimizer;` declarations, implement `optimize()`, `optimize_with_progress()`, `optimize_from_reader()` public APIs
- Key logic:
  - Add `pub mod trie; pub mod optimizer;`
  - `optimize(prefixes: &[IpNet], config: &OptimizerConfig) -> Result<OptimizationResult, OptimizeError>` — partition into IPv4/IPv6, run lossless, if target exceeded build trie + run optimizer, collect results with stats
  - `optimize_with_progress(...)` — same with progress callback + cancellation via `ControlFlow`
  - `optimize_from_reader(input: impl BufRead, config: &OptimizerConfig) -> Result<OptimizationResult, OptimizerError>` — parse then optimize
  - Config validation (max_prefix_len bounds, ratio in [0,1], target=0 with entries is error)
  - `compute_covered_ips_v4/v6()` — sum of `2^(32-prefix_len)` for each lossless entry
  - Build `OptimizationStats` with correct compression ratios and `target_binding` flags
  - `pub use` the new public API functions
- Dependencies: 1.1, 1.2, existing `lossless.rs`, `parser.rs`
- Testability: Integration test — `optimize(&[10.0.0.0/25, 10.0.0.128/25], config_with_target_1)` returns single `/24` with `over_coverage=0`; `optimize` with target < lossless count returns correct entry count; `optimize` with no target returns lossless result; `EmptyInput` error for empty slice

### 2.2 `crates/cidr-optimizer-cli/src/main.rs`
- Action: modify
- Purpose: Fix stats display bug, add `--validate` warning, wire lossy optimization via `optimize()` API
- Key logic:
  - Fix stats: use `parsed.ipv4.len()` for IPv4 input count, `parsed.ipv6.len()` for IPv6 input count
  - `--validate` flag: print `eprintln!("warning: --validate not yet implemented")` (full implementation deferred to Phase 3 provenance work)
  - Replace manual lossless calls with `optimize()` / `optimize_from_reader()` API call, passing `ipv4_target`, `ipv6_target`, `max_over_coverage` from CLI args into `OptimizerConfig`
  - Output `AggregatedEntry` results (already sorted by the library)
  - Stats now shows correct input/output counts from `OptimizationStats`
- Dependencies: 2.1
- Testability: `echo "10.0.0.0/25\n10.0.0.128/25" | cargo run -p cidr-optimizer-cli -- --ipv4-target 1` outputs `10.0.0.0/24`; `--stats` shows correct counts; `--validate` prints warning to stderr

## Phase 3 — Integrate
Deferred to future iterations.

### 3.1 Provenance extraction + output formats
- Files involved: `crates/cidr-optimizer/src/provenance.rs`, expand JSON/AWS output in CLI
- Dependencies: 2.1, 2.2
- Deferred details: Binary-search provenance mapping, full JSON format with `source_count`/`sources`/`over_coverage` per entry, `--validate` full implementation

### 3.2 Testing + fuzzing
- Files involved: `tests/`, `fuzz/`, property tests in `trie.rs` and `optimizer.rs`
- Dependencies: 2.1, 2.2
- Deferred details: Differential tests vs brute-force for small inputs, property tests for coverage invariant, fuzz targets, adversarial inputs

## Implementer Scope

Tasks the implementer MUST complete this iteration:
- 1.1, 1.2, 2.1, 2.2

Tasks deferred to future iterations:
- 3.1, 3.2

Rationale: 4 tasks covering the complete lossy optimization path (trie + optimizer + API + CLI wiring). Each is independently testable. The trie and optimizer are the most algorithmically complex pieces and benefit from focused attention. Keeping scope to 4 tasks avoids context overflow given the complexity of the 160-bit arithmetic and path compression logic.

## Testing Strategy
- `trie.rs`: inline unit tests verifying node size, build correctness, path compression, capacity computation, collapse mechanics
- `optimizer.rs`: inline unit tests verifying efficiency key ordering, ratio check, small-trie optimization to target
- `lib.rs`: inline unit tests verifying full `optimize()` pipeline (lossless-only, lossy with target, error cases)
- `cargo test --workspace` must pass with zero failures
- CLI smoke test: pipe known input with `--ipv4-target`, verify output count matches target

## Edge Cases
- Empty input → `OptimizeError::EmptyInput`
- Target already met by lossless → no trie built, lossless result returned directly
- Target = 1 → collapses to single /0 prefix (root), uses `u128::MAX` capacity
- All entries identical → lossless produces 1 entry, target always met
- IPv6 /128 scattered entries → path compression keeps node count manageable
- `max_over_coverage_ratio = 0.0` → no lossy merging allowed (stops immediately)
- Ratio check overflow → symmetric handling (LHS overflow: f64 fallback; RHS overflow: return false)
- Generation counter wrapping → safe because staleness uses `!=` comparison
- Arena overflow → `OptimizeError::ArenaOverflow` if trie exceeds `u32::MAX - 1` nodes
