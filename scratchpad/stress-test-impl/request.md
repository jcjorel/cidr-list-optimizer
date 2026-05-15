# Request: Implement Stress Test Suite

Implement the full stress test suite as specified in `scratchpad/cidr-optimizer-impl/stress-test-plan.md`.

## Summary

Implement deterministic stress tests for the CIDR optimizer library with:

1. **Phase 0**: Fix `validate_coverage` in `crates/cidr-optimizer/src/lib.rs` to use O(N log M) algorithm (binary search instead of nested loop)
2. **Phase 1**: Implement 7 test files (stress_sequential_v4, stress_sequential_v6, stress_subnets_v4, stress_nomerge_v4, stress_lossy_deterministic, stress_adversarial, stress_differential_v4v6)
3. **Phase 2**: Implement 2 test files (stress_mixed_and_provenance, stress_config_constraints)
4. **Helper module**: `tests/common/mod.rs` with shared utilities
5. **Feature gate**: Add `stress` feature to `crates/cidr-optimizer/Cargo.toml`

## Key Requirements

- All tests are deterministic (no randomness)
- Every test has 10K, 100K, AND 1M variants
- 100K/1M tests gated behind `#[cfg_attr(not(feature = "stress"), ignore)]`
- Every test asserts `validate_coverage(input, output) == true`
- Every test prints elapsed time via `eprintln!`
- Tests use only `ipnet` (already in dev-deps)

## Full Specification

See `scratchpad/cidr-optimizer-impl/stress-test-plan.md` for complete details.
