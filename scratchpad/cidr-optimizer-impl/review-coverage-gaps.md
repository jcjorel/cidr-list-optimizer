# Coverage Gap Review of Stress Test Plan

## Missing Test Scenarios (HIGH priority — could hide real bugs)

### Gap 1: Mixed IPv4+IPv6 inputs at scale
- What's missing: No test exercises `optimize()` with a mixed `Vec<IpNet>` containing both IPv4 and IPv6 entries simultaneously at scale. All tests are either pure-v4 or pure-v6.
- Why it matters: The `partition_with_indices` function splits inputs and the index mapping must remain correct across both families. The `build_result` function interleaves v4/v6 entries and sorts them — a bug in index tracking or sorting with mixed inputs would be invisible. The provenance code also iterates `result.entries` assigning v4/v6 provenance by counting — an off-by-one in the v4_idx/v6_idx tracking would corrupt all provenance for the second family.
- Suggested test: 5000 IPv4 /32s + 5000 IPv6 /128s, with provenance enabled. Assert that every `source_indices` entry maps back to the correct original input index and address family.

### Gap 2: `max_prefix_len` enforcement at scale
- What's missing: No stress test exercises `max_prefix_len_v4 < 32` or `max_prefix_len_v6 < 128` with large inputs. The lossless module truncates prefixes when `prefix_len > max_prefix_len`, which changes the coverage calculation and can create duplicates that then get merged via redundancy elimination.
- Why it matters: When `max_prefix_len_v4 = 24` and input contains 10000 /32s, ALL of them get truncated to /24s. This creates massive duplicate /24 entries that must be correctly deduplicated. The coverage recalculation (`cap = 1u128 << (32 - max_prefix_len)`) replaces the original coverage — if two /32s in the same /24 both get coverage set to 256, the redundancy elimination must handle this correctly. A bug here would produce incorrect over-coverage stats.
- Suggested test: 10000 /32s within a /16, with `max_prefix_len_v4 = 24`. Expected output: 256 /24s (or fewer if contiguous). Verify coverage sums are correct and no over-coverage is reported.

### Gap 3: `max_over_coverage_ratio` preventing target achievement
- What's missing: No test verifies the behavior when the ratio cap STOPS the optimizer before reaching the target. The optimizer's `exceeds_ratio` check breaks out of the loop early — but no stress test validates that: (a) the output count is > target, (b) the ratio is respected, (c) coverage is still maintained.
- Why it matters: The `exceeds_ratio` function has complex overflow-handling logic (integer-scaled 160-bit comparison). If it incorrectly returns `true` too early, the optimizer stops prematurely. If it incorrectly returns `false`, it over-merges. The stress test plan's lossy tests all use `None` for `max_over_coverage_ratio`.
- Suggested test: 10000 widely-spaced /32s (e.g., stride=65536 across the IPv4 space), target=10, `max_over_coverage_ratio = Some(0.01)`. The ratio cap should prevent reaching target=10 because each merge would add enormous over-coverage. Assert output_count > 10 AND total_over_coverage / input_covered_ips <= 0.01.

### Gap 4: Provenance correctness at scale
- What's missing: No stress test enables `config.provenance = true` and validates the `source_indices` mapping. The provenance code in `lib.rs` uses binary search on sorted input arrays and assigns indices — this is completely untested at scale.
- Why it matters: The provenance computation does `sorted_v4.sort_by_key(|(_, p)| u32::from(p.network()))` then uses binary search to map output prefixes back to input indices. With lossy optimization (where output prefixes are WIDER than inputs), the binary search boundaries must correctly capture all contained inputs. An off-by-one in the binary search would silently drop provenance entries.
- Suggested test: 10000 /32s with provenance=true, target=100. For each output entry, verify that `source_indices` is non-empty AND that every original input index appears in exactly one output entry's `source_indices`. The union of all `source_indices` must equal `0..10000`.

### Gap 5: Duplicate inputs (same CIDR repeated many times)
- What's missing: No test exercises duplicate prefixes. The lossless module's redundancy elimination handles containment but the behavior with exact duplicates depends on the sort stability and the `contains_v4` check (which returns true for equal prefixes since `outer.prefix_len() == inner.prefix_len()` and networks match).
- Why it matters: Looking at `redundancy_eliminate_v4`: when two identical prefixes appear, the second one hits `contains_v4(&top.prefix, &entry.prefix)` which returns TRUE (same prefix contains itself). So the duplicate's provenance gets merged into the first. But `coverage` doesn't change — this is correct for true duplicates but the source_indices accumulation could cause memory bloat or incorrect provenance if the same index appears multiple times. With 1000 duplicates of the same prefix, the `source_indices` vector grows to 1000 entries.
- Suggested test: 1000 copies of `10.0.0.0/24` + 1000 copies of `192.168.0.0/24`. Assert output = 2 entries, each with source_indices containing 1000 entries. Also test with provenance enabled to verify the full pipeline handles large source_indices vectors.

### Gap 6: CoverageLost validation never fires (negative test)
- What's missing: No test asserts that `validate_coverage()` returns true for ALL stress test outputs. The `CoverageLost` error path exists as a safety invariant but is never explicitly tested at scale. If a bug in the lossy optimizer produces an output that doesn't cover some input, this is the ONLY safety net.
- Why it matters: The `validate_coverage` function does O(input × output) containment checks. At scale (100K inputs × 100 outputs), this is 10M checks. If the function has a performance bug or an early-exit logic error, it might not catch a real coverage loss. More critically: if the lossy optimizer has a bug where it collapses a node that doesn't actually contain all its descendant leaves (e.g., due to path compression errors in `node_prefix_bits`), this is the only thing that catches it.
- Suggested test: Every stress test should explicitly call `validate_coverage(input, &result.entries)` and assert true. Additionally, construct a deliberately broken output (remove one entry) and assert `validate_coverage` returns false — proving the safety net works.

### Gap 7: Single-entry edge cases (N=1)
- What's missing: No test exercises N=1 input with various configurations: target=1, target=None, provenance=true, max_prefix_len truncation on a single entry.
- Why it matters: The lossless module has `if entries.len() <= 1 { return; }` in `sibling_merge_v4` — correct. But the trie builder with a single leaf, the optimizer with `remaining <= target` on first check, and the provenance binary search with a single output entry are all degenerate cases that could have off-by-one bugs.
- Suggested test: Single /32 input with: (a) no target, (b) target=1, (c) provenance=true, (d) max_prefix_len=24 (forces truncation). Assert correct output in each case.

### Gap 8: Cancellation at scale
- What's missing: No test exercises the `optimize_with_progress` cancellation path during actual lossy optimization of large inputs. The progress callback is called at phase transitions but the test plan doesn't verify that returning `ControlFlow::Break(())` during the `Phase::Lossy` callback actually stops processing and returns `Err(OptimizeError::Cancelled)`.
- Why it matters: If the cancellation check is only at phase boundaries (which it is — see `lib.rs` lines calling `progress()`), then a long-running lossy optimization of 100K entries cannot be cancelled mid-computation. This is a UX/safety issue. But more subtly: if cancellation fires between lossless_v4 and lossless_v6, the partial state is discarded — but is there a resource leak?
- Suggested test: 100K entries, target=10, cancel on `Phase::Lossy` callback. Assert `Err(OptimizeError::Cancelled)` is returned. Also cancel on `Phase::Lossless { af: IPv6 }` with mixed input to verify partial v4 results are properly dropped.

## Nice-to-Have Scenarios (MEDIUM priority)

### Gap 9: ArenaOverflow near u32::MAX
- What's missing: No test approaches the `u32::MAX` node limit in the trie arena. The `alloc_node` function returns `Err(OptimizeError::ArenaOverflow)` when `idx == INVALID` (u32::MAX).
- Why it matters: With path compression, each /32 input creates ~2-3 nodes (not 32). So hitting u32::MAX (~4 billion nodes) would require ~1.5 billion inputs at ~80 bytes/node = ~320GB RAM. This is impractical to test directly. However, a targeted test could verify the error path fires correctly with a mock or by checking the arithmetic.
- Suggested test: Unit test that manually fills arena to u32::MAX - 1 entries and verifies the next `alloc_node()` returns `ArenaOverflow`. This doesn't need to be a stress test — just a correctness test of the overflow check.

### Gap 10: Power-of-two boundary inputs (2^N entries)
- What's missing: The test plan uses 10K, 100K, 1M — but never exactly 2^N entries (1024, 2048, 4096, 8192, 16384, 32768, 65536). These are interesting because contiguous /32 ranges of exactly 2^N always merge to a single prefix.
- Why it matters: The binary decomposition logic and trie structure have natural power-of-two boundaries. Edge cases at exactly these boundaries (where the entire input collapses to one prefix) test the full cascade depth. The 1M test (1048576 = 2^20) does cover this, but smaller powers (2^10, 2^16) would catch issues faster.
- Suggested test: For each N in {10, 12, 14, 16}: generate 2^N contiguous /32s, assert output = exactly 1 prefix.

### Gap 11: IPv6 with very short prefixes (/0, /1, /2)
- What's missing: No test exercises extremely short IPv6 prefixes where coverage values approach u128::MAX. The `compute_covered_ips_v6` function has special handling: `if (128 - pl) >= 128 { u128::MAX }`.
- Why it matters: A /0 prefix has coverage u128::MAX. If two /1 prefixes are input, they should merge to /0 with coverage u128::MAX (saturating add). The `saturating_add` in `compute_covered_ips_v6` and the `exceeds_ratio` overflow handling are both exercised only with these extreme values.
- Suggested test: Input = `::/1` + `8000::/1`. Assert output = `::/0` with coverage = u128::MAX. Also test with `max_over_coverage_ratio` to exercise the large-value ratio comparison.

### Gap 12: Lossy optimization where target equals current count
- What's missing: No test exercises the edge case where `lossless_v4.len() == target` (target is exactly met by lossless). The code checks `lossless_v4.len() > target` — so if equal, lossy is skipped.
- Why it matters: This is a boundary condition. If lossless produces exactly N entries and target=N, the optimizer should NOT enter lossy mode. A `>=` vs `>` bug would cause unnecessary lossy processing.
- Suggested test: Generate inputs that produce exactly 100 entries after lossless, set target=100. Assert no lossy processing occurs (over_coverage = 0, target_binding = false).

### Gap 13: Heap compaction correctness in optimizer
- What's missing: The optimizer has a heap compaction path: `if heap.len() > 4 * remaining`. No test specifically triggers this code path at scale.
- Why it matters: The compaction filters stale entries by checking `!n.is_leaf && n.generation == *g`. If this filter is wrong (e.g., keeps stale entries or drops valid ones), the optimizer could skip valid merges or attempt invalid ones. This path is only triggered when many collapses have occurred, creating many stale heap entries.
- Suggested test: Input designed to trigger many small collapses (e.g., 10000 sibling pairs scattered across the address space, target=1). This forces many collapses with many stale entries, triggering compaction.

## Adequately Covered
- Sequential contiguous /32 merging (IPv4 and IPv6) — well covered at 10K/100K/1M
- Full subnet population and cascading merges — covered by stress_subnets_v4
- Non-mergeable inputs (worst-case trie size) — covered by stress_nomerge_v4
- Basic lossy optimization with targets — covered by stress_lossy_deterministic
- Adversarial patterns (dense, alternating, nested, sibling-only) — covered by stress_adversarial
- Differential testing against ipnet::aggregate — covered by stress_differential_v4v6
- Performance baselines with timing — all tests print elapsed time
- Coverage invariant assertion — mentioned in principles but needs explicit enforcement (see Gap 6)
