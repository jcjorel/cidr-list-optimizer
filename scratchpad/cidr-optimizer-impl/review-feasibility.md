# Feasibility Review of Stress Test Plan

## Blocking Issues (plan cannot work as written)

### Issue 1: `validate_coverage` is O(N×M) — catastrophically slow at scale

- **Problem**: The `validate_coverage` function in `lib.rs` is:
  ```rust
  pub fn validate_coverage(input: &[IpNet], output: &[AggregatedEntry]) -> bool {
      input.iter().all(|inp| {
          output.iter().any(|out| out.prefix.contains(inp))
      })
  }
  ```
  This is O(input_count × output_count). For the 1M no-merge test (Test File 4), output = input = 1M entries → **1 trillion comparisons**. Even for 100K inputs × 10K outputs = 1 billion comparisons. Each comparison involves IP parsing/masking operations.
- **Impact**: The 1M no-merge test will never complete. Even the 100K no-merge test will take hours. The `validate_coverage` call happens **inside the library itself** (in `optimize_with_progress`), not just in tests — so this blocks all large-scale usage.
- **Fix**: The stress tests should NOT rely on the library's internal `validate_coverage` for correctness at scale. Options:
  1. Sort both input and output by network address, then use a sweep-line algorithm: O(N log N + M log M)
  2. Build a trie from output prefixes, then check each input against the trie: O(N × 32) for IPv4
  3. For tests specifically: disable the internal `validate_coverage` call via config flag for large inputs, and use a smarter validation in the test harness
  4. **Most critical**: The library itself will be unusable for >10K entries with the no-merge case. This is a library bug, not just a test issue.

### Issue 2: `tests/common/mod.rs` pattern does NOT work for integration tests

- **Problem**: Rust compiles each file in `tests/` as a separate crate. A `tests/common/mod.rs` file is compiled as its **own integration test binary** named `common`, not as a shared module. The compiler will warn about dead code and attempt to run it as a test.
- **Impact**: The shared helper module won't be importable from other test files using `mod common;`. Compilation will fail or produce warnings.
- **Fix**: Use `tests/common.rs` with `mod common;` in each test file — this doesn't work either. The correct Rust pattern is:
  - `tests/helpers/mod.rs` (directory-based module) — also doesn't work
  - **Correct approach**: Create `tests/common/mod.rs` inside a directory: the path must be `tests/common/mod.rs` and each test file uses `#[path = "common/mod.rs"] mod common;` — OR more idiomatically, put helpers in a `tests/support.rs` file and reference it as `mod support;` from each test. Actually, the **standard pattern** is: create `tests/common.rs` (NOT `mod.rs`) and in each test file write `mod common;`. Wait — that doesn't work either because each test is its own crate root.
  - **Actually correct**: The idiomatic Rust pattern is `tests/common/mod.rs` (as a directory). Each test file then does `mod common;` which resolves to `tests/common/mod.rs`. This DOES work because `cargo test` only compiles files directly in `tests/` as test crates, not subdirectories. So `tests/common/mod.rs` is fine — it won't be compiled as a standalone test. **The plan is actually correct.**
  - **Correction to my correction**: Files directly in `tests/*.rs` are test crates. `tests/common/mod.rs` inside a subdirectory is NOT auto-discovered as a test crate. Each test file can `mod common;` and Rust will find `tests/common/mod.rs`. This is the documented pattern. **Plan is feasible.**

### Issue 3: The library's internal `validate_coverage` makes ALL large inputs infeasible

- **Problem**: Looking at `optimize_with_progress` in `lib.rs`:
  ```rust
  // Safety invariant: output MUST cover all input prefixes
  if !validate_coverage(prefixes, &result.entries) {
      return Err(OptimizeError::CoverageLost);
  }
  ```
  This runs on EVERY call to `optimize()`. For the no-merge case with 100K entries: 100K × 100K = 10 billion comparisons. For sequential 1M entries that merge to 1 prefix: 1M × 1 = 1M comparisons (fine). But for the no-merge case, it's quadratic.
- **Impact**: Test File 4 (`stress_nomerge_v4`) at 100K entries will timeout due to the library's own internal validation, not the test's assertions. The test can't even call `optimize()` successfully within time budget.
- **Fix**: Either:
  1. Make `validate_coverage` use binary search (sort output by network, binary search for containing prefix) — O(N log M)
  2. Add a config flag to skip internal validation for trusted callers
  3. Accept that no-merge tests at 100K+ are infeasible until the library is fixed

---

## Performance Concerns (may exceed time/memory budgets)

### Concern 1: Memory for 1M IPv6 /128s in the trie

- **Analysis**: With path compression, 1M sequential /128s starting from `2001:db8::0` share a 108-bit common prefix (since 2^20 = 1M, only 20 bits vary). The trie structure:
  - ~1M leaf nodes (one per /128)
  - ~1M internal nodes (binary tree over 20 varying bits)
  - Total: ~2M nodes × 80 bytes = **160 MB**
- **Verdict**: Feasible on 8GB CI. But note: during lossless aggregation, the `radix_sort_v6` creates a `key_bytes: Vec<[u8; 17]>` of 1M entries = 17MB, plus the entries themselves (each `ProvenancePrefix<Ipv6Net>` with a `Vec<usize>` source_indices = ~56 bytes minimum) = ~56MB for entries. Total working memory ~250-300MB. **Feasible but tight if other processes are running.**
- **However**: The lossless step will merge all 1M sequential /128s into a single /108 BEFORE the trie is built. The trie only receives the lossless output. So for sequential inputs, the trie gets 1 entry, not 1M. Memory is fine.
- **Worst case for trie memory**: Test File 4 (no-merge) — 100K non-adjacent /32s. Trie gets 100K entries. With path compression, ~200K nodes × 80 bytes = 16MB. Fine.

### Concern 2: Radix sort for 1M IPv6 entries (17 passes)

- **Analysis**: `radix_sort_v6` does 17 counting-sort passes over the data. For 1M entries: 17 × 1M = 17M operations. Each pass also allocates a `key_bytes` array of 17MB and an `indices_buf` of 8MB.
- **Verdict**: Should complete in ~1-2 seconds. Feasible.

### Concern 3: Sibling merging for 1M sequential entries

- **Analysis**: `sibling_merge_v6` sorts by (prefix_len DESC, network ASC) then does a stack-based merge. For 1M sequential /128s, the sort is O(N log N) = ~20M comparisons. The cascading merge will produce log2(1M) ≈ 20 merge levels, each halving the count. Total work: ~2M merge operations.
- **Verdict**: Should complete in 2-5 seconds. Feasible.

### Concern 4: Lossy test (Test File 5) with evenly-spaced /32s

- **Analysis**: For N=10000 evenly-spaced /32s with target=100: lossless produces 10000 entries (no merging possible since they're non-adjacent). The trie is built with 10000 leaves. The optimizer heap processes ~10000 internal nodes. O(N log N) = ~130K operations. Fine.
- **For N=100K with target=100**: Trie with 100K leaves, heap with ~100K entries. O(100K × log(100K)) ≈ 1.7M operations. Should complete in <5 seconds.
- **BUT**: The internal `validate_coverage` will be O(100K × 100) = 10M comparisons. Each is an IP contains check. This should take ~1-2 seconds. Borderline but feasible.

### Concern 5: `ipnet::aggregate()` time complexity for differential tests

- **Analysis**: `Ipv4Net::aggregate()` and `Ipv6Net::aggregate()` in the `ipnet` crate use a sort + merge approach. Looking at ipnet's source, it sorts the input, removes redundancies, then merges siblings iteratively. Time complexity: O(N log N) for sort + O(N × max_prefix_len) for iterative merging.
- **For 100K entries**: O(100K × log(100K)) ≈ 1.7M for sort + O(100K × 32) ≈ 3.2M for merge passes. Should complete in <2 seconds.
- **Verdict**: Feasible for 100K. Would be slow for 1M but no 1M differential test is planned.

### Concern 6: Generation of evenly-spaced /32s (Test File 5)

- **Analysis**: stride = 2^32 / 100000 ≈ 42949. Addresses: 0, 42949, 85898, ... Each is a valid IPv4 address (just a u32 converted to Ipv4Addr). Each /32 is a single host — they cannot overlap by definition. The last address: 99999 × 42949 = 4,294,857,051 < 2^32 = 4,294,967,296. Valid.
- **Concern**: Integer overflow for large N. For N=100K: max address = 99999 × 42949 = 4,294,857,051 which fits in u32. Fine.
- **Verdict**: Feasible and produces valid non-overlapping /32s.

---

## Confirmed Feasible

- **Sequential IPv4 tests (Test File 1)**: 1M contiguous /32s will merge to 1 prefix via lossless aggregation. The lossless step is O(N log N) for sort + O(N) for merge. Trie receives 1 entry. `validate_coverage` is O(1M × 1) = 1M ops. Total time: ~5-10 seconds for 1M. Feasible.
- **Sequential IPv6 tests (Test File 2)**: Same analysis as IPv4. Lossless merges 1M /128s to 1 /108. Feasible.
- **Full subnets test (Test File 3)**: 4096 /24s × 256 /32s = 1M inputs. Lossless merges each 256 /32s into a /24, then merges /24s. Final output: 1 /12. `validate_coverage` is O(1M × 1). Feasible.
- **Adversarial tests (Test File 6)**: All sub-tests use ≤65536 inputs. Feasible.
- **Differential tests (Test File 7)**: 10K-100K scale. `ipnet::aggregate` handles this fine. Feasible.
- **The `minimal_prefix_decomposition` helper**: This is the count of 1-bits in the binary representation of the range endpoints, computed by decomposing a range [start, start+count) into aligned power-of-2 blocks. Algorithm: repeatedly find the largest power-of-2 aligned block that fits within the remaining range. Well-defined and O(log N) per call. Formula: count the number of set bits in `count` when `start` is 0-aligned, otherwise iteratively subtract the largest aligned block. Implementable.
- **`tests/common/mod.rs` pattern**: Works correctly. Cargo only auto-discovers `tests/*.rs` files as test crates, not files in subdirectories.
- **Generation patterns**: All mathematically sound and produce valid inputs.

---

## Recommendations

1. **Critical fix needed**: Replace `validate_coverage` in the library with an O(N log M) algorithm before running stress tests. Without this, Test File 4 (no-merge) is completely blocked at any scale above 10K, and Test File 5 (lossy) is borderline at 100K.

2. **Gating strategy for ignored tests**: Instead of `#[ignore]` (never runs in CI), use:
   - A cargo feature flag: `#[cfg_attr(not(feature = "stress"), ignore)]`
   - Run `cargo test --features stress` in a weekly/nightly CI job
   - This ensures regressions are caught within a week, not never

3. **The `minimal_prefix_decomposition` function**: For `start=0`, this is simply `count.count_ones()` when count is a power of 2, but for arbitrary ranges it's more complex. The correct algorithm:
   ```
   fn minimal_prefix_decomposition(start: u32, count: u32) -> usize {
       let mut remaining = count;
       let mut pos = start;
       let mut num_prefixes = 0;
       while remaining > 0 {
           // Largest power-of-2 block aligned at `pos` that fits in remaining
           let alignment = if pos == 0 { u32::MAX } else { pos.trailing_zeros() };
           let max_block = 1u32 << alignment.min(31);
           let block = max_block.min(remaining.next_power_of_two() >> (if remaining.is_power_of_two() { 0 } else { 0 }));
           // Actually: largest power of 2 ≤ remaining that is aligned at pos
           let block_size = (1u32 << pos.trailing_zeros().min(31)).min(remaining.next_power_of_two() / if remaining.is_power_of_two() { 1 } else { 2 }).max(1);
           // Simpler: find largest 2^k where pos % 2^k == 0 and 2^k <= remaining
           let mut k = pos.trailing_zeros().min(31);
           while (1u32 << k) > remaining { k -= 1; }
           let block_size = 1u32 << k;
           pos += block_size;
           remaining -= block_size;
           num_prefixes += 1;
       }
       num_prefixes
   }
   ```
   This is well-defined but the implementation needs care. For the special case of `start=0` and `count` being a power of 2, the answer is always 1.
