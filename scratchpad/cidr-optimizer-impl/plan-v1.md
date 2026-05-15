---
iteration: 1
status: complete
files_count: 8
---

# Implementation Plan v1

## Summary
Create the foundational crate structure, types, error handling, parser, and lossless aggregation engine for the CIDR list optimizer. This iteration delivers a compiling workspace with a working lossless aggregation pipeline (Phase 2 of the implementation plan).

## Dependency Graph
```
types.rs ← error.rs ← parser.rs ← lossless.rs ← lib.rs
                                                     ↑
crates/cidr-optimizer/Cargo.toml ←──────────────────┘
crates/cidr-optimizer-cli/Cargo.toml ← main.rs (skeleton)
```

## Phase 1 — Scaffold
Small tasks. Each independently verifiable.

### 1.1 `crates/cidr-optimizer/Cargo.toml`
- Action: create
- Purpose: Library crate manifest with dependencies
- Key logic: Declare `ipnet = "2"`, `thiserror = "2"` deps; `proptest = "1"` as dev-dep
- Dependencies: none
- Testability: `cargo check -p cidr-optimizer` succeeds (with empty lib.rs)

### 1.2 `crates/cidr-optimizer/src/types.rs`
- Action: create
- Purpose: All public types — `OptimizerConfig`, `AggregatedEntry`, `OptimizationResult`, `OptimizationStats`, `AddressFamily`, `Phase`
- Key logic: Struct definitions with `Default` impl for `OptimizerConfig` (max_prefix_len_v4=32, v6=128, max_input_entries=10M)
- Dependencies: 1.1
- Testability: `cargo check -p cidr-optimizer` compiles; unit test asserts `OptimizerConfig::default()` field values

### 1.3 `crates/cidr-optimizer/src/error.rs`
- Action: create
- Purpose: `OptimizeError` and `OptimizerError` enums with `thiserror` derives
- Key logic: Split error types per spec §6 — `OptimizeError` (no Parse), `OptimizerError` (includes Parse, Io, wraps OptimizeError)
- Dependencies: 1.1
- Testability: `cargo check`; unit test constructs each variant and formats via `Display`

### 1.4 `crates/cidr-optimizer/src/parser.rs`
- Action: create
- Purpose: Parse input lines into partitioned IPv4/IPv6 prefix vectors with indices
- Key logic: `ParsedInput` struct; `parse_input(impl BufRead, store_strings: bool, max_entries: usize)` — skip comments/blanks, normalize via `trunc()`, warn on non-canonical, error on `max_entries` exceeded
- Dependencies: 1.2, 1.3
- Testability: Unit tests — parse valid IPs, CIDRs, comments, blanks, non-canonical warning, max_entries error

### 1.5 `crates/cidr-optimizer/src/lib.rs`
- Action: create
- Purpose: Module declarations and minimal public re-exports
- Key logic: `mod types; mod error; mod parser;` + `pub use` of public API types. Note: `mod lossless;` is added in Task 2.1 when that module is created.
- Dependencies: 1.2, 1.3, 1.4
- Testability: `cargo build -p cidr-optimizer` compiles cleanly

## Phase 2 — Build
Medium tasks. Each builds on Phase 1 and is testable at completion.

### 2.1 `crates/cidr-optimizer/src/lossless.rs`
- Action: create
- Purpose: Sort-based lossless aggregation for IPv4 and IPv6
- Key logic: (a) Radix sort by (network_addr, prefix_len) — 5 passes IPv4, 17 passes IPv6. (b) Monotone-stack redundancy elimination with pop-loop. (c) `max_prefix_len` enforcement (truncate, re-sort, re-dedup). (d) Stack-based sibling merging with cascading. (e) `ProvenancePrefix<N>` carries `source_indices: Vec<usize>`. Also adds `mod lossless;` to `lib.rs`.
- Dependencies: 1.2, 1.3, 1.5
- Testability: Unit tests — redundancy removal (`/8` subsumes `/16`), sibling merge (`10.0.0.0/25 + 10.0.0.128/25 → 10.0.0.0/24`), cascading merge, max_prefix_len truncation, provenance tracking through merges. Differential test vs `ipnet::aggregate()` for random inputs.

### 2.2 `crates/cidr-optimizer-cli/Cargo.toml` + `crates/cidr-optimizer-cli/src/main.rs`
- Action: create
- Purpose: CLI binary skeleton with clap argument parsing
- Key logic: Clap derive struct with all flags from spec §7. Main reads stdin/file, calls `parse_input`, calls `lossless_aggregate_v4`/`v6`, prints plain output. No lossy optimization yet.
- Dependencies: 1.5, 2.1
- Testability: `cargo build -p cidr-optimizer-cli` compiles; `echo "10.0.0.0/25\n10.0.0.128/25" | cargo run -p cidr-optimizer-cli` outputs `10.0.0.0/24`

## Phase 3 — Integrate
Coarser tasks. Detailed breakdown deferred to next iteration if needed.

### 3.1 Path-compressed trie + greedy optimizer
- Files involved: `crates/cidr-optimizer/src/trie.rs`, `crates/cidr-optimizer/src/optimizer.rs`
- Dependencies: 2.1 (lossless output feeds trie construction)
- Deferred details: Arena-allocated trie with 80-byte nodes, path compression, greedy collapse with BinaryHeap cost-efficiency key, widening 160-bit multiplication, over-coverage tracking, ratio check

### 3.2 Full pipeline + provenance + output formats
- Files involved: `crates/cidr-optimizer/src/provenance.rs`, `crates/cidr-optimizer/src/lib.rs` (expand), `crates/cidr-optimizer-cli/src/main.rs` (expand)
- Dependencies: 3.1
- Deferred details: `optimize()` / `optimize_with_progress()` APIs, JSON/AWS output, `--validate` flag, progress callback

## Implementer Scope

Tasks the implementer MUST complete this iteration:
- 1.1, 1.2, 1.3, 1.4, 1.5, 2.1, 2.2

Tasks deferred to future iterations:
- 3.1, 3.2

Rationale: 7 tasks covering the full foundation (types, errors, parser, lossless aggregation, CLI skeleton). Each is independently testable. This delivers a working lossless-mode tool end-to-end, which is the prerequisite for the lossy optimizer in the next iteration. The lossless module is the most algorithmically dense piece in this scope and benefits from focused attention.

## Testing Strategy
- Each module has inline `#[cfg(test)] mod tests` with unit tests
- `lossless.rs` includes a differential test comparing output against `ipnet::aggregate()` (add `ipnet` to dev-dependencies with its `aggregate` feature)
- `cargo test --workspace` must pass with zero failures
- CLI smoke test: pipe known input, verify expected output

## Edge Cases
- Empty input → `OptimizeError::EmptyInput`
- Input exceeding `max_input_entries` → `OptimizeError::InputTooLarge`
- Non-canonical CIDR (`10.0.0.5/24`) → normalized to `10.0.0.0/24` with warning
- Duplicate entries → deduplicated, provenance tracks all original indices
- Overlapping CIDRs (`10.0.0.0/8` + `10.1.0.0/16`) → redundancy elimination keeps `/8`
- All entries identical → single output entry
- Single entry input → passes through unchanged
- `max_prefix_len` shorter than input → truncation creates duplicates that must be re-merged
- IPv6 /128 entries → lossless handles correctly (radix sort on 17 bytes)
