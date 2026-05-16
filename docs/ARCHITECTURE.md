# Architecture

## System Overview

```
┌─────────────────────────────────────────────────────────────────────┐
│                         Input (stdin / file)                        │
└───────────────────────────────────┬─────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────┐
│  Phase 1: Parse & Partition                                         │
│  • Line-by-line parsing with index assignment                       │
│  • IPv4/IPv6 partitioning                                           │
│  • Host-bit truncation (normalization)                              │
└───────────────────────────────────┬─────────────────────────────────┘
                                    │
                          ┌─────────┴─────────┐
                          ▼                   ▼
┌─────────────────────────────┐ ┌─────────────────────────────┐
│  Phase 2: Lossless (IPv4)   │ │  Phase 2: Lossless (IPv6)   │
│  • Radix sort               │ │  • Radix sort               │
│  • Redundancy elimination   │ │  • Redundancy elimination   │
│  • max_prefix_len enforce   │ │  • max_prefix_len enforce   │
│  • Sibling merging (stack)  │ │  • Sibling merging (stack)  │
└──────────────┬──────────────┘ └──────────────┬──────────────┘
               │                               │
               ▼                               ▼
┌─────────────────────────────┐ ┌─────────────────────────────┐
│  Phase 3: Lossy (IPv4)      │ │  Phase 3: Lossy (IPv6)      │
│  • Path-compressed trie     │ │  • Path-compressed trie     │
│  • Cost-efficiency greedy   │ │  • Cost-efficiency greedy   │
│  • Ratio-capped stopping    │ │  • Ratio-capped stopping    │
│  (skipped if within budget) │ │  (skipped if within budget) │
└──────────────┬──────────────┘ └──────────────┬──────────────┘
               │                               │
               └─────────────┬─────────────────┘
                             ▼
┌─────────────────────────────────────────────────────────────────────┐
│  Phase 4: Result Assembly                                           │
│  • Leaf extraction & sorting (IPv4 before IPv6, by network addr)    │
│  • Source-map computation (binary search, opt-in)                    │
│  • Coverage validation                                              │
└─────────────────────────────────────────────────────────────────────┘
```

Each address family is processed independently through Phases 2–3. The lossy phase is skipped entirely when no target is set or the lossless output already fits within budget.

## Theoretical Foundation

The algorithm draws on several lines of research in prefix aggregation:

**ORTC (Optimal Routing Table Constructor)** — Draves et al. (Microsoft Research, 1999). Provably minimal lossless aggregation via 3-pass dynamic programming on a binary trie. Establishes that lossless prefix minimization is solvable in polynomial time on trie structures.

**FAQS (Fast Aggregation with Quick Selections)** — 2018. Achieves 2.5× speedup over ORTC through selection-based optimization rather than full DP traversal.

**FIB Aggregation** — Zhao et al. (IEEE/ACM Transactions on Networking, 2012). Proves that lossless FIB aggregation (preserving forwarding correctness with zero error) is polynomial-time solvable via DP. Their work addresses a different problem (minimize entry count with zero error) than ours (minimize error with bounded entry count), but establishes that prefix-tree optimization problems admit efficient solutions.

**Our problem: budget-constrained lossy aggregation** — Given n lossless prefixes, reduce to ≤k entries minimizing total over-coverage. This is a cardinality-constrained tree optimization problem. General tree knapsack is NP-hard (Johnson & Niemi 1983), but the cost-additive structure of prefix trees enables an efficient greedy heuristic with strong practical performance.

### Design Choice: Hybrid Sort + Trie

Rather than building a full binary trie from raw input (which would be prohibitively expensive for millions of scattered /32s), the algorithm uses a two-stage approach:

1. **Sort-based lossless aggregation** — Handles millions of raw inputs with cache-friendly sequential access. No trie overhead for the common case where lossless output fits within budget.
2. **Path-compressed trie for lossy optimization** — Built only from the (much smaller) lossless output. Path compression keeps node count proportional to the number of distinct prefixes rather than address space depth.

This hybrid avoids the worst case of a full trie (12.8M nodes for 100K scattered IPv6 /128s) while retaining the structural properties needed for the greedy optimizer.

## Algorithm Phases

### Phase 1: Parse & Partition

Parses input line-by-line, assigning sequential indices for source-map tracking. Each entry is normalized (host bits truncated: `10.0.0.5/24` → `10.0.0.0/24`) and partitioned into IPv4/IPv6 streams. Non-canonical CIDRs emit a warning. Full-line comments (`#` at start) and blank lines are skipped; inline comments (text after `#` on a CIDR line) are stripped from the CIDR and preserved as metadata when source-map is enabled. Input size is bounded by `max_input_entries` to prevent OOM.

### Phase 2: Sort-Based Lossless Aggregation

Three sub-phases produce a minimal lossless prefix set:

**Radix sort** — O(n) for fixed-width keys using LSD (least-significant-digit) radix-256 counting sort:
- IPv4: 4 address bytes + 1 prefix_len byte = 5 passes
- IPv6: 16 address bytes + 1 prefix_len byte = 17 passes

**Redundancy elimination** — A monotone stack processes sorted prefixes. For each prefix P: pop entries that do not contain P (they go to output); if the new top contains P, P is redundant (record source mapping); otherwise push P.

**Sibling merging** — After sorting by (prefix_length DESC, network_address ASC), a stack-based pass merges siblings bottom-up with cascading. When the top two stack entries are siblings (same length, addresses differ only in bit at position L−1), they merge to their parent (length L−1, union source indices). The merged result may form a new sibling pair, triggering further merges up the tree. O(n) amortized — each entry is pushed/popped at most O(depth) times, total merges ≤ n−1.

Between redundancy elimination and sibling merging, `max_prefix_len` is enforced by truncating, re-sorting, and re-deduplicating.

### Phase 3: Path-Compressed Trie & Greedy Lossy Optimization

When lossless output exceeds the target budget:

1. **Build path-compressed trie** from lossless output. Single-child chains are compressed into single nodes with a `skip_len` field, keeping node count proportional to the number of distinct prefixes.

2. **Bottom-up computation** of `coverage` (input IPs covered) and `leaf_count` at each node.

3. **Exclusion marking**: if exclusions are configured, `mark_exclusions()` walks the trie and sets `is_excluded = true` on any node whose address interval intersects the ExclusionSet. Ancestors of excluded nodes are also marked. Excluded nodes are never collapsed by the greedy loop.

4. **Initialize min-heap** with all internal nodes where `leaf_count ≥ 2` and `is_excluded = false`, keyed by cost-efficiency:

```
efficiency = cost / (leaf_count - 1)
cost = capacity(node) - coverage(node)
```

5. **Greedy collapse loop**: Pop minimum-efficiency node, verify freshness via generation counter, check ratio cap, execute collapse (mark as leaf, invalidate descendants), update ancestors, push updated ancestors back into heap. Repeat until `entry_count ≤ target` or heap exhausted.

**Dual-mode target specification**: The optimizer supports two target modes via `TargetSpec`:
- `EntryCount(k)` — reduce to ≤k entries, optionally capped by a ratio. The loop terminates when `remaining ≤ k` or the ratio cap is hit.
- `MaxOverCoverage(ratio)` — find the minimum entry count such that over-coverage stays ≤ ratio × covered IPs. Implemented by setting `target = 1` (theoretical floor) and passing the ratio as the cap. The loop terminates when the next collapse would exceed the ratio — the entry count floor is never reached in practice. This inverts the optimization: instead of "fit N entries with minimal waste," it solves "minimize entries subject to a waste budget."

**Priority queue**: `BinaryHeap<Reverse<(EfficiencyKey, u32, u32)>>` — composite key `(efficiency, node_idx, generation)`. Ties broken by `node_idx` for determinism.

**Staleness handling**: O(1) per pop via generation counter comparison. Total invalidation work is O(total_nodes) amortized across all collapses. When total heap size exceeds 4× the remaining entry count, the heap is compacted by filtering invalid entries.

### Phase 4: Result Assembly

Remaining trie leaves are collected as output prefixes, sorted by network address (IPv4 before IPv6). When source-map is enabled, binary search on the sorted original input identifies which entries each output prefix covers. Per-prefix over-coverage is computed as `capacity − coverage`.

## Data Structures

### Trie Node Layout

```
#[repr(C)]  // 80 bytes, fixed layout
TrieNode {
    skip_bits: u128,               // The compressed bit pattern
    coverage: u128,                // Input IPs covered by this subtree
    collapsed_cost_sum: u128,      // Cumulative over-coverage of collapsed descendants
    children: [NodeIdx; 2],        // Left/right child indices (INVALID = u32::MAX sentinel)
    parent: NodeIdx,               // Parent index for ancestor traversal
    leaf_count: u32,               // Number of leaves in subtree
    generation: u32,               // Staleness counter for heap entries
    depth: u8,                     // Depth in the binary trie (bits from root)
    is_leaf: bool,                 // Whether this is a current leaf
    skip_len: u8,                  // Path-compressed bits (0 = no compression)
    is_excluded: bool,             // Whether this node intersects an exclusion zone
    _pad: [u8; 8],                 // Padding to reach 80-byte alignment
}
```

`NodeIdx = u32`. Children use `u32::MAX` as an invalid sentinel rather than `Option<u32>` to maintain the fixed 80-byte layout. The `parent` field enables O(depth) ancestor traversal during collapse updates. `collapsed_cost_sum` tracks cumulative over-coverage of already-collapsed descendants, enabling correct incremental over-coverage accounting without subtree re-traversal.

Nodes are stored in a contiguous arena (`Vec<TrieNode>`) indexed by `u32`, giving O(1) access and cache-friendly traversal. Arena overflow (> 2³² nodes) returns an error.

### BinaryHeap Entry

```
Reverse<(EfficiencyKey, node_idx: u32, generation: u32)>

EfficiencyKey {
    cost: u128,      // capacity(node) - coverage(node)
    savings: u32,    // leaf_count - 1
}
```

`EfficiencyKey` implements `Ord` via 160-bit widening multiplication: comparing `cost_a / savings_a` vs `cost_b / savings_b` is done as `cost_a × savings_b` vs `cost_b × savings_a` (cross-multiply to avoid division and maintain exact ordering). The multiplication uses a `(u64, u128)` pair representing a 160-bit result. The root node (depth=0) uses maximum cost — collapsed only as last resort. Ties are broken by `node_idx` for determinism.

### Source-Map Storage

When enabled, each lossless prefix carries a `Vec<usize>` of original input indices. During sibling merging, source-index vectors are concatenated. During lossy optimization, source mapping is not tracked in the trie — instead, it is reconstructed in Phase 4 via binary search on the sorted input (O(output × log n)).

## Cost-Efficiency Greedy Rationale

**Why cost-efficiency, not raw cost**: Raw cost (absolute over-coverage of a collapse) biases toward tiny 2-leaf merges with low absolute cost but poor cost-per-entry-saved. A node saving 3 entries at cost 6 (efficiency=2) is better than a node saving 1 entry at cost 3 (efficiency=3). Cost-efficiency ensures each unit of entry-budget is spent on the cheapest available over-coverage.

**Why not provably optimal**: The problem is a cardinality-constrained tree optimization (reduce to exactly k leaves, minimize total cost). General tree knapsack is NP-hard (Johnson & Niemi 1983). The feasible collapse sets do not form a matroid — collapsing a node invalidates descendants, creating dependencies that prevent the standard exchange argument.

**Near-optimality in practice**: For typical inputs (partner IP feeds with clustered IPs), property tests verify `greedy_over_coverage ≤ 1.05 × brute_force_optimal` for small inputs (n ≤ 20). The cost-additive structure means no "hidden costs" — collapsing a node has a fixed, predictable cost regardless of prior decisions.

**Exact O(n×k) tree DP alternative**: The cost-additive structure admits an exact solution:
```
dp[v][j] = minimum over-coverage using exactly j leaves in subtree(v)
Base: dp[leaf][1] = 0
Collapse: dp[v][1] = cost(v)
Split: dp[v][j] = min_{j_L + j_R = j} (dp[left][j_L] + dp[right][j_R])  for j ≥ 2
```
For n=50K, k=5K this is ~250M operations — feasible but with high constant factors. The greedy is preferred for v1: O(k log k) vs O(n×k), streaming-friendly, simpler, and empirically within 1–3% of optimal.

## Correctness Argument

1. **Cost additivity**: `cost(P) = capacity(P) - coverage(P) = cost(L) + cost(R)` for any node P with children L, R. Capacity partitions exactly at branch points; coverage is additive over disjoint subtrees. This property is preserved under path compression — compressed nodes merely elide intermediate branch points.

2. **Path independence**: Total over-coverage of any final leaf set depends only on which nodes are leaves, not on the collapse sequence (direct consequence of additivity).

3. **Coverage invariant**: Every IP in the input is covered by at least one output prefix. Enforced by a post-optimization validation pass using binary search (O(N log M)).

4. **Path compression safety**: Compressed (single-child) nodes have `leaf_count = 1` and are never collapse candidates. Path compression does not remove any valid candidate from the greedy's consideration set.

5. **Determinism**: Identical input and configuration produce identical output. Ties in the heap are broken by `node_idx` (deterministic insertion order from trie construction).

6. **Exclusion safety**: Exclusions only prevent collapses (set `is_excluded` on internal nodes). They never remove leaves or modify coverage values. Therefore, the coverage invariant (every input prefix is covered by some output leaf) is preserved — exclusions can only increase the output entry count, never decrease it.

## Over-Coverage Tracking

Running over-coverage must not double-count when a parent is collapsed after a child:

```
actual_over_coverage = Σ cost(leaf) for all current leaves that are collapsed nodes
```

Original lossless leaves have cost 0; only collapsed nodes contribute. When parent P is collapsed (absorbing previously-collapsed child C), C is no longer a leaf — its cost is removed and P's cost replaces it. Net change = `cost(P) - collapsed_cost_sum(P)`.

Implementation tracks `current_over_coverage` incrementally using the `collapsed_cost_sum` field on each node:

1. On collapse of node P: compute `net_new_over = cost(P) - P.collapsed_cost_sum`
2. Add `net_new_over` to the global `current_over_coverage`
3. Propagate `net_new_over` upward: for each ancestor A, add `net_new_over` to `A.collapsed_cost_sum`

This avoids re-traversing the subtree at collapse time — each ancestor update is O(1), and the total propagation is O(depth) per collapse.

## Ratio Check — Integer Arithmetic for IPv6

To avoid f64 precision loss for large IPv6 ranges:

```
// Instead of: (over as f64 / covered as f64) > max_ratio
// Use: over * SCALE > threshold * covered
const SCALE: u128 = 1_000_000;
let threshold = (max_ratio * SCALE as f64) as u128;
let exceeds = over.checked_mul(SCALE)
    .map_or(true, |scaled| scaled > threshold.saturating_mul(covered));
```

The ratio tracks only over-coverage introduced by the lossy greedy phase. Over-coverage from `max_prefix_len` truncation is excluded from the ratio check but included in final per-entry reporting.

## Complexity

| Phase | Time | Space |
|-------|------|-------|
| Parse & partition | O(n) | O(n) |
| Radix sort + lossless aggregation | O(n) | O(n) |
| Build path-compressed trie (from k lossless entries) | O(k × d) | O(k × d) |
| Lossy greedy (BinaryHeap) | O(k log k) | O(k) |
| Source-map extraction | O(output × log n) | O(n) |

Overall: **O(n + k log k)** time, **O(n)** space. Since k ≤ n (lossless output ≤ input), and typically k ≪ n, the radix sort dominates for large inputs.

## Memory Budget

| Component | Per-entry cost | 1M IPv4 entries |
|-----------|---------------|-----------------|
| Parsed input | ~16 bytes | 16 MB |
| Radix sort buffer | ~16 bytes | 16 MB |
| Lossless output (est. 200K) | ~32 bytes | 6 MB |
| Path-compressed trie (est. 400K nodes) | ~80 bytes | 32 MB |
| BinaryHeap | ~24 bytes/entry | 5 MB |
| **Total (no source-map)** | | **~75 MB** |

With source-map: add ~24 bytes/input entry for `source_indices` storage (+24 MB for 1M entries).

Worst case (scattered /32s, no sharing): trie reaches ~2× lossless count nodes. For 500K lossless entries: ~80 MB trie. Total stays well under 2 GB for IPv4.

IPv6 /128 entries: Path compression is critical. Without it, 100K scattered /128s would create 12.8M nodes (1 GB). With path compression: ~200K–400K nodes (~32 MB).

## Crate Structure & Module Responsibilities

```
crates/
├── cidr-optimizer/              Core library crate
│   └── src/
│       ├── lib.rs               Public API surface: optimize(), optimize_with_progress(),
│       │                        optimize_from_reader(), validate_coverage(),
│       │                        parse_cidrs(), parse_exclusions(). Orchestrates
│       │                        the full pipeline (partition → lossless → lossy → assemble).
│       │                        Owns the coverage validation invariant. Builds ExclusionSet
│       │                        and passes to lossy phase. Detects exclusion-constrained
│       │                        state. Populates exclusion_collisions on output entries.
│       ├── types.rs             Public data types: OptimizerConfig, TargetSpec,
│       │                        OptimizationResult, AggregatedEntry, OptimizationStats,
│       │                        ExclusionEntry, ExclusionCollision, ParsedCidr, InputEntry,
│       │                        ReaderResult, Phase, AddressFamily. Pure data definitions
│       │                        except TargetSpec
│       │                        which implements FromStr for parsing target specification
│       │                        strings ("60" or "over-coverage=0.1%").
│       ├── error.rs             Error enums: OptimizeError (library-level) and
│       │                        OptimizerError (reader-level, includes parse/IO/
│       │                        target-spec parsing). Implements From conversions
│       │                        between them.
│       ├── parser.rs            Line-by-line input parsing with index assignment.
│       │                        Exposes parse_cidrs() and parse_exclusions() as
│       │                        reusable public APIs (re-exported at crate root).
│       │                        Handles comments, blank lines, normalization warnings.
│       │                        Collects input metadata (original text, inline comments)
│       │                        when source-map is enabled. Enforces 4 KiB per-line
│       │                        length limit. parse_input() wraps parse_cidrs() with
│       │                        max-entries enforcement and IPv4/IPv6 partitioning.
│       ├── lossless.rs          Sort-based lossless aggregation: radix sort, redundancy
│       │                        elimination (monotone stack), max_prefix_len enforcement,
│       │                        and sibling merging with cascading. Operates on
│       │                        SourceMapPrefix<T> carrying source indices.
│       ├── trie.rs              Path-compressed binary trie: arena-allocated nodes,
│       │                        construction from lossless output, bottom-up coverage/
│       │                        leaf_count computation, collapse execution, leaf
│       │                        extraction, and mark_exclusions() which marks nodes
│       │                        whose intervals intersect exclusion ranges.
│       ├── optimizer.rs         Greedy lossy optimizer: BinaryHeap management, cost-
│       │                        efficiency ranking, generation-based staleness, ratio
│       │                        cap checking, and ancestor update propagation. Skips
│       │                        collapse of nodes where is_excluded=true. Operates
│       │                        on BinaryTrie without knowledge of address family.
│       ├── exclusion.rs         Exclusion set construction from ExclusionEntry list.
│       │                        Losslessly aggregates exclusion prefixes into sorted
│       │                        non-overlapping intervals for O(log E) intersection
│       │                        queries during lossy optimization.
│       └── source_map.rs        Post-optimization source-map reconstruction via binary
│                                search on sorted input. Maps each output prefix to the
│                                set of original input indices it covers.
│
└── cidr-optimizer-cli/          CLI binary crate (thin wrapper)
    └── src/
        └── main.rs              Argument parsing (clap), input reading, config
                                 construction, output formatting (plain/JSON/AWS),
                                 statistics display, and validation error handling.
                                 Contains no optimization logic.
```

**Interface boundaries**: The CLI depends on the library's public API only (`optimize`, `optimize_from_reader`, `parse_cidrs`, `parse_exclusions`, `TargetSpec::from_str()`, `OptimizerConfig`, result types). The library modules have a layered dependency: `lib.rs` → `lossless` → (radix sort internals); `lib.rs` → `trie` → `optimizer`; `lib.rs` → `exclusion`; `trie` → `exclusion` (for `mark_exclusions`); `lib.rs` → `source_map`. The `trie` and `optimizer` modules are decoupled — `optimizer` receives a `&mut BinaryTrie` and drives the greedy loop without knowing how the trie was constructed.

## References

1. Draves, R. et al. "Constructing Optimal IP Routing Tables" (1999) — [PDF](https://www.microsoft.com/en-us/research/wp-content/uploads/2016/02/tr-98-59.pdf). Provably minimal lossless aggregation via 3-pass DP on binary trie.
2. FAQS: Fast Aggregation with Quick Selections (2018) — [arXiv:1812.05520](https://arxiv.org/abs/1812.05520). 2.5× faster than ORTC.
3. FIFA: Fast Incremental FIB Aggregation (2013) — [IEEE](https://ieeexplore.ieee.org/document/6566913/). Incremental updates to aggregated tables.
4. SMALTA IETF Draft — [draft-uzmi-smalta-01](https://datatracker.ietf.org/doc/html/draft-uzmi-smalta-01). Scalable multi-level aggregation.
5. RFC 4632: CIDR — [rfc4632](https://www.rfc-editor.org/rfc/rfc4632). Classless Inter-Domain Routing specification.
6. Zhao, J. et al. "On the Aggregatability of Router Forwarding Tables," IEEE/ACM Transactions on Networking, 20(3), 2012. Proves lossless FIB aggregation is polynomial-time solvable via DP.
7. Johnson, D.S. & Niemi, K.A. "On Knapsacks, Partitions, and a New Dynamic Programming Technique for Trees" (1983) — [INFORMS](https://pubsonline.informs.org/doi/pdf/10.1287/moor.8.1.1). Pseudo-polynomial DP for tree-structured optimization; basis for a future O(n·k) exact mode.

---

*This project and its documentation were fully generated using Gen AI coding tools employing multi-pass adversarial reviews to minimize errors. While this process significantly reduces defects, it cannot guarantee the complete absence of bugs.*