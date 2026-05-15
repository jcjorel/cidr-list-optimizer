# Feedback from Iteration 2

## Minor Issues to Fix

### 1. Over-coverage stats report capacity instead of net over-coverage
- File: `crates/cidr-optimizer/src/lib.rs`
- For trie-extracted entries with empty `source_indices`, `coverage_from_sources()` returns 0, so over-coverage = full capacity instead of actual over-coverage.
- Fix: Use the optimizer's internal `collapsed_cost_sum` tracking to compute correct over-coverage, or implement provenance extraction from the trie so source_indices are populated for collapsed entries.

### 2. Clippy type_complexity warning
- File: `crates/cidr-optimizer/src/lib.rs:142`
- Fix: Add `#[allow(clippy::type_complexity)]` or extract a type alias.

## Next Phase Direction

Iterations 1-2 successfully implemented the full optimization pipeline (lossless + lossy). Iteration 3 should complete the project:

1. **Provenance module** (`provenance.rs`) — Track which original CIDRs contributed to each output entry. This fixes the over-coverage reporting issue.

2. **`--validate` flag implementation** — After optimization, verify that all original IPs are still covered by the output set. Report any gaps.

3. **Testing** — Property tests (using proptest or quickcheck), integration tests with realistic CIDR lists, differential tests comparing lossless output against `ipnet::aggregate()`.

4. **Output formats** — Ensure JSON and AWS output formats include provenance information as specified in `scratchpad/specification.md`.

Refer to `scratchpad/specification.md` §5 (Provenance) and §6 (Output Formats) and `scratchpad/implementation.md` Phase 4-5 for details.
