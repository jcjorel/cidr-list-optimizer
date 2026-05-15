# CIDR List Optimizer — High-Level Specification (v5)


## 1. Overview

**cidr-list-optimizer** is a Rust library (primary deliverable) with an associated CLI tool that takes an arbitrarily large list of IPv4/IPv6 addresses and CIDR prefixes and produces an optimized, smaller list of CIDR prefixes that fits within a configurable entry budget while minimizing over-coverage (IP addresses included in the output that were not present in the input).

Each output CIDR maintains **provenance**: a mapping back to the original input entries it encompasses.

## 2. Motivation & Use Cases

AWS networking services impose hard limits on the number of IP filtering entries:

| AWS Service | Resource | Limit | Source |
|-------------|----------|-------|--------|
| Security Groups | Rules per SG per direction per AF | 60 (adjustable, SG×rules ≤ 1000) | [VPC Quotas](https://docs.aws.amazon.com/vpc/latest/userguide/amazon-vpc-limits.html) |
| Managed Prefix Lists | Entries per prefix list | 1,000 | [VPC Quotas](https://docs.aws.amazon.com/vpc/latest/userguide/amazon-vpc-limits.html) |
| WAF IP Sets | IPs per IP set | 10,000 (fixed) | [WAF Quotas](https://docs.aws.amazon.com/waf/latest/developerguide/limits.html) |
| Network Firewall | Rule group capacity | 30,000 | [Network Firewall Quotas](https://docs.aws.amazon.com/network-firewall/latest/developerguide/quotas.html) |
| Network ACLs | Rules per NACL | 20 (max 40) | [VPC Quotas](https://docs.aws.amazon.com/vpc/latest/userguide/amazon-vpc-limits.html) |

**Typical scenario**: A threat intelligence feed provides 500,000+ malicious IP addresses/CIDRs. These must be distilled into ≤1,000 prefix list entries for use in Security Groups, accepting minimal over-coverage as a tradeoff.

## 3. Functional Requirements

### 3.1 Input

- A file (or stdin) containing one IP address or CIDR prefix per line
- Mixed IPv4 and IPv6 entries in the same input
- Supported formats:
  - Single IP: `192.168.1.1`, `2001:db8::1`
  - CIDR: `10.0.0.0/8`, `2001:db8::/32`
  - Host CIDR: `192.168.1.1/32`, `2001:db8::1/128`
- Lines starting with `#` are comments; blank lines are ignored
- Input size: up to tens of millions of entries (bounded by `max_input_entries` config)

### 3.2 Output

- A list of CIDR prefixes sorted by network address ascending (IPv4 before IPv6)
- Fits within the configured entry budget
- Each output entry optionally carries **provenance**: which input entries it covers
- Output is deterministic for the same input and configuration

### 3.3 Provenance Tracking (Opt-In)

When enabled (`--provenance` or `config.provenance = true`), each output CIDR reports which original input entries it encompasses:

```
Output: 10.0.0.0/22
  Sources: [10.0.0.0/24, 10.0.1.0/24, 10.0.2.5, 10.0.3.0/25]
  Over-coverage: 384 IPs not in original input
```

This enables:
- Auditing which threat intel entries map to which firewall rules
- Understanding the "cost" of each aggregation decision
- Incremental updates (if an input entry is removed, which output CIDRs are affected)

### 3.4 Optimization Objectives

**Primary objective**: Minimize the number of output entries to fit within the target budget.

**Secondary objective**: Among all solutions that fit the budget, minimize total over-coverage (number of IPs in output CIDRs that were NOT in any input entry).

### 3.5 Configuration Parameters

| Parameter | Description | Default |
|-----------|-------------|---------|
| `ipv4_target` | Maximum IPv4 output entries | None (lossless) |
| `ipv6_target` | Maximum IPv6 output entries | None (lossless) |
| `max_over_coverage_ratio` | Maximum acceptable `over_coverage / input_covered_ips` (per AF) | None |
| `max_prefix_len_v4` | Maximum prefix length for IPv4 output | 32 |
| `max_prefix_len_v6` | Maximum prefix length for IPv6 output | 128 |
| `max_input_entries` | Maximum input entries before returning error | 10,000,000 |
| `provenance` | Enable provenance tracking | false |

### 3.6 Target Budget Semantics

- Budgets are **per address family**. Each AF is optimized independently.
- `--ipv4-target N`: IPv4 output entries ≤ N.
- `--ipv6-target N`: IPv6 output entries ≤ N.
- If a target is not specified for an AF, that AF runs in lossless mode (no lossy merging).
- Minimum target per AF: 1. Target 0 is an error if that AF has entries (vacuously satisfied if no entries for that AF).

### 3.7 Operating Modes

1. **Lossless mode**: Standard CIDR aggregation (no over-coverage). Equivalent to `aggregate6`/`rs-aggregate`.
2. **Budget mode**: Reduce to target entry count, minimizing over-coverage.
3. **Ratio-capped mode**: Merge until budget is met OR `over_coverage / input_covered_ips` exceeds threshold.

### 3.8 `max_over_coverage_ratio` Definition

```
over_coverage_ratio = af_over_coverage_ips / af_input_covered_ips
```

- Computed **per address family** independently.
- Denominator is the total number of distinct IPs covered by the original input for that AF (fixed, computed once).
- Ratio 0.0 = lossless only. Ratio 0.05 = output may cover up to 5% more IPs than the input for that AF.
- If the ratio cap is reached before the target, merging stops early (output may exceed target count).
- **Scope**: The ratio tracks only over-coverage introduced by the **lossy greedy phase**. Over-coverage introduced by `max_prefix_len` truncation (e.g., /32 inputs truncated to /24 each cover 255 extra IPs) is NOT counted toward this ratio. The final `over_coverage` field in output entries reports the TOTAL over-coverage (including truncation-induced), but the ratio cap governs only the greedy's decisions.
- **Precision**: For IPv6 ranges larger than /48, the ratio check uses integer-scaled arithmetic to avoid f64 precision loss (see §4.3).

## 4. Algorithm

### 4.1 Theoretical Foundation

- **ORTC (Optimal Routing Table Constructor)** — Draves et al. (Microsoft Research, 1999). Provably minimal lossless aggregation via 3-pass DP on binary trie.
  - PDF: https://www.microsoft.com/en-us/research/wp-content/uploads/2016/02/tr-98-59.pdf

- **FAQS (Fast Aggregation with Quick Selections)** — 2018. 2.5x faster than ORTC.
  - Paper: https://arxiv.org/abs/1812.05520

- **FIB aggregation literature** — Zhao et al. (2012) study lossless FIB aggregation (preserving forwarding correctness with zero error) and prove it polynomial-time solvable via DP. Their work addresses a different problem (minimize entry count with zero error) than ours (minimize error with bounded entry count), but establishes that prefix-tree optimization problems can admit efficient solutions.
  - Paper: Zhao, J. et al. "On the Aggregatability of Router Forwarding Tables," IEEE/ACM Transactions on Networking, 20(3), 2012.

- **Our problem: budget-constrained lossy aggregation** — Given n lossless prefixes, reduce to ≤ k entries minimizing total over-coverage. This is a cardinality-constrained tree optimization problem. General tree knapsack is NP-hard (Johnson & Niemi 1983), but our cost-additive structure enables an efficient greedy heuristic with strong practical performance. An exact O(n×k) tree DP exists (see §4.6) but the greedy is preferred for its simplicity and streaming-friendliness at scale.

- **Greedy heuristic with cost-efficiency ranking** — The greedy selects collapses by minimum cost per entry saved (cost-efficiency). Key structural properties:
  1. **Cost additivity**: `cost(P) = capacity(P) - coverage(P) = cost(L) + cost(R)`. Total over-coverage depends only on the final leaf set, not collapse order.
  2. **Cost-efficiency metric**: Ranking by `cost / (leaf_count - 1)` ensures each unit of budget (entry reduction) is spent on the cheapest available over-coverage.
  3. **Empirical near-optimality**: For typical blocklist inputs (clustered IPs, power-law prefix distributions), the greedy produces results within 1-3% of optimal (verified via brute-force on small inputs in property tests).

### 4.2 Algorithm Overview — Hybrid Approach

1. **Sort-based lossless aggregation** (no trie) — handles millions of raw inputs efficiently
2. **Path-compressed trie-based lossy optimization** (built from lossless output only) — greedy collapse on the smaller set

### 4.3 Algorithm Phases

#### Phase 1: Parse & Partition
- Parse all input entries, assign sequential indices (for provenance)
- Reject input if entry count exceeds `max_input_entries` (default 10M)
- Partition into IPv4 and IPv6 streams
- Normalize: truncate host bits (e.g., `10.0.0.5/24` → `10.0.0.0/24`); emit warning for non-canonical CIDRs

#### Phase 2: Sort-Based Lossless Aggregation
- **Radix sort** prefixes by (network_address, prefix_length) — O(n) for fixed-width keys
  - IPv4: 4 address bytes + 1 prefix_len byte = **5 passes** of radix-256 counting sort
  - IPv6: 16 address bytes + 1 prefix_len byte = **17 passes** of radix-256 (LSD radix sort, one byte per pass). Alternative: **9 passes** of radix-65536 (2 bytes per pass) for better throughput at the cost of 512KB histogram per pass.
- **Redundancy elimination** (monotone stack with pop-loop):
  - For each prefix P in sorted order:
    - Pop from stack while stack top does NOT contain P (popped entries go to output)
    - If new top contains P → P is redundant (record provenance)
    - Else → push P
- **Enforce max_prefix_len**: Truncate, re-sort, re-deduplicate
- **Stack-based sibling merging** (bottom-up with cascading):
  - Sort by (prefix_length DESC, network_address ASC)
  - Process entries using a stack: push each entry; when the top two stack entries are siblings (same length, addresses differ only in bit at position L-1), pop both, merge to parent (length L-1, union provenance), push merged result back. Repeat until top two are not siblings.
  - Cascading: a merge at /24→/23 may create a new sibling pair at /23, triggering further merges up the tree — all handled by the stack re-check loop.
  - O(n) amortized — each original entry is pushed/popped at most O(depth) times, but total merges ≤ n-1.
- Track provenance: each merged prefix records which original indices it absorbed.

#### Phase 3: Path-Compressed Trie-Based Lossy Optimization (Greedy)
If lossless result exceeds target budget for that AF:

```
Build path-compressed trie from lossless output
Compute coverage and leaf_count at each node (bottom-up)

Initialize BinaryHeap (min-heap by efficiency key) with ALL internal nodes where leaf_count >= 2:
  efficiency = cost / (leaf_count - 1)       // compared via widening 160-bit multiplication for exact transitive ordering
  cost = capacity(node) - coverage(node)     // u128
  For root (depth=0): efficiency = u128::MAX (saturating) — collapsed only as last resort

While entry_count > target AND heap not empty:
    Pop (efficiency, node_idx, generation) from heap
    If node.generation != stored generation OR node.is_leaf: skip (stale)
    
    Compute net_new_over_coverage for ratio check (see §4.4)
    Check max_over_coverage_ratio using integer arithmetic: if exceeded, stop early
    
    Execute collapse: mark node as leaf
    Invalidate all descendant nodes (increment their generation)
    entry_count -= (leaf_count - 1)
    current_over_coverage += net_new_over_coverage
    Update ancestor leaf_counts and efficiencies; push ancestors with new keys into heap
```

**Priority queue**: `std::collections::BinaryHeap<Reverse<(u128, u32, u32)>>` — composite key `(efficiency, node_idx, generation)`. This provides:
- O(log k) insert/extract-min (where k = number of candidates, typically ≤ 500K)
- **Structural determinism**: ties broken by `node_idx` (deterministic insertion order)
- **Cost-efficiency ranking**: ensures each unit of entry-budget is spent on the cheapest over-coverage, not biased toward small collapses

**Staleness**: O(1) per pop via generation counter comparison. Total invalidation work is O(total_nodes) amortized across all collapses.

**Why cost-efficiency, not raw cost**: Raw cost biases toward tiny 2-leaf merges (low absolute cost but poor cost-per-entry). A node saving 3 entries at cost 6 (efficiency=2) is better than a node saving 1 entry at cost 3 (efficiency=3). Cost-efficiency ensures budget is spent optimally per unit.

#### Phase 4: Extract Results with Provenance
- Collect remaining trie leaves as output prefixes
- Sort output by network address ascending
- If provenance enabled: binary search on sorted original input to find which entries each output covers
- Compute per-prefix over-coverage = `2^(addr_bits - prefix_len) - sum_of_input_ips_covered`

### 4.4 Over-Coverage Tracking (Correct Accounting)

The running over-coverage must NOT double-count when a parent is collapsed after a child:

```
actual_over_coverage = Σ cost(leaf) for all current leaves that are collapsed nodes
```

Since original lossless leaves have cost 0, only collapsed nodes contribute. When a parent P is collapsed (absorbing a previously-collapsed child C), C is no longer a leaf — so its cost is removed from the sum and P's cost replaces it. Net change = `cost(P) - cost(C)`.

**Implementation**: Track `current_over_coverage` and on each collapse of node P:
- Subtract costs of any already-collapsed descendant leaves within P's subtree
- Add cost(P)

This ensures the ratio check uses the true over-coverage of the current leaf set.

### 4.5 Ratio Check — Integer Arithmetic for IPv6

To avoid f64 precision loss for large IPv6 ranges:

```rust
// Instead of: (over as f64 / covered as f64) > max_ratio
// Use scaled integer comparison:
// over * SCALE > (max_ratio * SCALE) as u128 * covered
const SCALE: u128 = 1_000_000; // 6 decimal places of precision
let threshold = (max_ratio * SCALE as f64) as u128;
let exceeds = over.checked_mul(SCALE)
    .map_or(true, |scaled_over| scaled_over > threshold.saturating_mul(covered));
```

For IPv4 (where values fit in u64), f64 is exact and this optimization is unnecessary.

### 4.6 Algorithm Properties & Correctness Argument

The greedy heuristic has strong structural properties that make it highly effective in practice:

1. **Cost additivity**: `cost(P) = capacity(P) - coverage(P) = cost(L) + cost(R)` for any node P with children L, R. This holds because `capacity(P) = capacity(L) + capacity(R)` and `coverage(P) = coverage(L) + coverage(R)`. **This property is preserved under path compression**: compressed (single-child) nodes merely elide intermediate branch points; at every actual branch point, the address space still partitions exactly into two disjoint halves, so capacity and coverage remain additive over children regardless of skip_len.

2. **Path independence**: The total over-coverage of any final leaf set depends only on which nodes are leaves, not on the sequence of collapses that produced them (direct consequence of additivity).

3. **Cost-efficiency greedy**: By ranking candidates by `cost / (leaf_count - 1)`, the greedy spends each unit of entry-budget on the cheapest available over-coverage. This is the natural greedy strategy for the fractional relaxation of the problem.

4. **Why not provably optimal**: The problem is a cardinality-constrained tree optimization (reduce to exactly k leaves, minimize total cost). General tree knapsack is NP-hard (Johnson & Niemi 1983). The feasible collapse sets do not form a matroid — a collapse invalidates descendants, creating dependencies that prevent the standard exchange argument from applying. However:

5. **Near-optimality in practice**: For typical inputs (threat intel feeds with clustered IPs):
   - Cost-efficiency ranking avoids the pathological case where raw-cost greedy wastes budget on tiny low-impact merges
   - The cost-additive structure means no "hidden costs" — collapsing a node has a fixed, predictable cost regardless of prior decisions
   - Property tests verify `greedy_over_coverage ≤ 1.05 × brute_force_optimal` for small inputs (n ≤ 20)

6. **Path compression safety**: Compressed (single-child) nodes have `leaf_count = 1` and are never collapse candidates. Therefore path compression does not remove any valid candidate from the greedy's consideration set.

**Formal context**: Zhao et al. (IEEE/ACM ToN, 2012) prove that lossless FIB aggregation is polynomial-time solvable via DP. Our problem (lossy, budget-constrained) is different but benefits from the same cost-additive tree structure. Johnson & Niemi (1983) provide DP techniques for tree-structured optimization; a future version could implement O(n·k) tree DP for provably optimal results at the cost of higher runtime.

**Exact O(n×k) tree DP alternative**: The cost-additive structure admits an exact solution via tree DP:
```
dp[v][j] = minimum over-coverage using exactly j leaves in subtree(v)
Base: dp[leaf][1] = 0 (original leaves have zero over-coverage)
Collapse: dp[v][1] = cost(v)
Split: dp[v][j] = min_{j_L + j_R = j} (dp[left][j_L] + dp[right][j_R])  for j ≥ 2
```
Using the knapsack merge optimization (merging DP arrays at each node), total work is O(n×k) where n = lossless output size and k = target. For n=50K, k=5K this is ~250M operations — feasible but with high constant factors (random-access DP table, poor cache locality). **The greedy is preferred for v1** because: (a) O(k log k) vs O(n×k) runtime for typical inputs, (b) streaming-friendly (can report progress/cancel mid-optimization), (c) simpler implementation with fewer edge cases, (d) empirically within 1-3% of optimal for real-world blocklist distributions. The DP is a candidate for a future `--exact` mode.

### 4.7 Complexity

| Phase | Time | Space |
|-------|------|-------|
| Parse | O(n) | O(n) |
| Radix sort + lossless | O(n) | O(n) |
| Build path-compressed trie (from lossless output k) | O(k × d) | O(k × d) |
| Lossy greedy (BinaryHeap) | O(k log k) | O(k) |
| Provenance extraction | O(output × log n) | O(n) |

Overall: **O(n + k log k)** time, **O(n)** space. Since k ≤ n (lossless output ≤ input), and typically k << n, the radix sort O(n) dominates for large inputs.

### 4.8 Memory Budget

| Component | Per-entry cost | 1M IPv4 entries |
|-----------|---------------|-----------------|
| Parsed input | ~16 bytes | 16 MB |
| Radix sort buffer | ~16 bytes | 16 MB |
| Lossless output (est. 200K) | ~32 bytes | 6 MB |
| Path-compressed trie (est. 400K nodes) | ~80 bytes | 32 MB |
| BinaryHeap | ~24 bytes/entry | 5 MB |
| **Total (no provenance)** | | **~75 MB** |

With provenance enabled, add ~24 bytes/input entry for `source_indices` storage: +24 MB.

**Worst case** (scattered /32s, no sharing): trie may reach ~2× lossless count nodes. For 500K lossless entries: ~80 MB trie. Total stays well under 2 GB for IPv4.

**IPv6 /128 entries**: Path compression is critical. Without it, 100K scattered /128s would create 12.8M nodes (1 GB). With path compression, this reduces to ~200K-400K nodes (~32 MB).

## 5. Non-Functional Requirements

### 5.1 Performance
- Handle 1M+ input entries in < 5 seconds on modern hardware
- Memory usage < 4GB for 10M entries (greedy mode, either AF)
- Memory usage bounded: ~68 bytes/IPv4 entry, ~84 bytes/IPv6 entry (typical)
- Lossless phase: sort-based, cache-friendly, no trie overhead

### 5.2 Correctness
- Every IP in the input MUST be covered by at least one output prefix (no false negatives)
- Over-coverage is minimized via cost-efficiency greedy heuristic (near-optimal for typical inputs; see §4.6)
- Deterministic output: ties broken by `(efficiency, node_idx)` composite key in BinaryHeap
- `max_prefix_len` constraint always respected
- Optional `--validate` flag verifies coverage invariant on output

### 5.3 Robustness
- Graceful handling of duplicate input entries
- Overlapping input CIDRs handled correctly (union semantics, all tracked for provenance)
- Invalid lines produce warnings but don't abort processing
- Non-canonical CIDRs (e.g., `10.0.0.5/24`) normalized with warning
- IPv6 root node (true capacity = 2^128, represented as u128::MAX = 2^128−1; off-by-one is 1 part in 2^128, negligible) handled via saturating arithmetic (collapsed only as last resort when target=1)
- Input size bounded by `max_input_entries` to prevent OOM
- Arena overflow check: error if trie exceeds u32::MAX nodes

## 6. Library API (Primary Deliverable)

```rust
use ipnet::IpNet;
use std::ops::ControlFlow;

pub struct OptimizerConfig {
    /// Maximum IPv4 output entries. None = lossless for IPv4.
    pub ipv4_target: Option<usize>,
    /// Maximum IPv6 output entries. None = lossless for IPv6.
    pub ipv6_target: Option<usize>,
    /// Stop merging if over_coverage / input_covered_ips exceeds this (per AF).
    pub max_over_coverage_ratio: Option<f64>,
    pub max_prefix_len_v4: u8,   // default: 32
    pub max_prefix_len_v6: u8,   // default: 128
    /// Maximum input entries (returns error if exceeded). Default: 10M.
    pub max_input_entries: usize, // default: 10_000_000
    /// Enable provenance tracking (increases memory usage).
    pub provenance: bool,        // default: false
}

/// Progress information passed to callback
pub enum Phase {
    Parsing { entries_read: usize },
    Lossless { af: AddressFamily, entries_remaining: usize },
    Lossy { af: AddressFamily, current_count: usize, target: usize },
    Done,
}

/// A single output prefix with optional provenance
pub struct AggregatedEntry {
    pub prefix: IpNet,
    /// Indices into the original input (None if provenance disabled)
    pub source_indices: Option<Vec<usize>>,
    /// Number of IPs in this prefix NOT covered by any input entry
    pub over_coverage: u128,
}

pub struct OptimizationResult {
    pub entries: Vec<AggregatedEntry>,
    pub stats: OptimizationStats,
}

pub struct OptimizationStats {
    pub input_ipv4_count: usize,
    pub input_ipv6_count: usize,
    pub output_ipv4_count: usize,
    pub output_ipv6_count: usize,
    pub total_ipv4_over_coverage: u128,
    pub total_ipv6_over_coverage: u128,
    pub ipv4_compression_ratio: f64,
    pub ipv6_compression_ratio: f64,
    /// Whether the target was binding (lossless output exceeded target)
    pub ipv4_target_binding: bool,
    pub ipv6_target_binding: bool,
}

/// Primary library API: optimize from pre-parsed prefixes.
pub fn optimize(
    prefixes: &[IpNet],
    config: &OptimizerConfig,
) -> Result<OptimizationResult, OptimizeError>;

/// Iterator-based API: accepts any IntoIterator, partitions and collects internally.
/// Avoids requiring the caller to build a Vec<IpNet>, but still allocates O(n) internally.
pub fn optimize_iter(
    prefixes: impl IntoIterator<Item = IpNet>,
    config: &OptimizerConfig,
) -> Result<OptimizationResult, OptimizeError>;

/// With progress callback and cancellation support.
pub fn optimize_with_progress(
    prefixes: &[IpNet],
    config: &OptimizerConfig,
    progress: impl FnMut(Phase) -> ControlFlow<()>,
) -> Result<OptimizationResult, OptimizeError>;

/// Convenience: parse and optimize from a reader (for CLI-like usage).
pub fn optimize_from_reader(
    input: impl BufRead,
    config: &OptimizerConfig,
) -> Result<OptimizationResult, OptimizerError>;
```

**Error types** (split for API clarity):

```rust
/// Errors from optimize() / optimize_iter() — no Parse variant
pub enum OptimizeError {
    EmptyInput,
    InvalidConfig { message: String },
    TargetTooSmall { target: usize, minimum: usize },
    InputTooLarge { count: usize, limit: usize },
    ArenaOverflow,
    Cancelled,
}

/// Errors from optimize_from_reader() — includes parsing
pub enum OptimizerError {
    Optimize(OptimizeError),
    Parse { line: usize, message: String },
    Io(std::io::Error),
}
```

## 7. CLI Interface (Thin Wrapper)

```bash
# Per-AF budgets (each AF optimized independently)
cidr-optimizer --ipv4-target 800 --ipv6-target 200 input.txt

# IPv4 only budget (IPv6 stays lossless)
cidr-optimizer --ipv4-target 1000 input.txt

# Stdin
cat feed.txt | cidr-optimizer --ipv4-target 500

# Lossless aggregation only (no target specified)
cidr-optimizer input.txt

# Show provenance (which inputs map to each output)
cidr-optimizer --ipv4-target 1000 --provenance input.txt

# Show statistics on stderr
cidr-optimizer --ipv4-target 1000 --stats input.txt

# Cap over-coverage at 5% per AF
cidr-optimizer --ipv4-target 1000 --max-over-coverage 0.05 input.txt

# Validate output covers all inputs (self-check)
cidr-optimizer --ipv4-target 1000 --validate input.txt

# Output formats
cidr-optimizer --ipv4-target 1000 --format plain input.txt
cidr-optimizer --ipv4-target 1000 --format json input.txt
cidr-optimizer --ipv4-target 1000 --format aws input.txt
```

## 8. Output Formats

### Plain (default)
One CIDR per line, sorted by network address ascending:
```
10.0.0.0/22
192.168.0.0/16
2001:db8::/32
```

### JSON (with provenance when enabled)
```json
{
  "ipv4": [
    {
      "prefix": "10.0.0.0/22",
      "source_count": 4,
      "sources": ["10.0.0.0/24", "10.0.1.0/24", "10.0.2.5/32", "10.0.3.0/25"],
      "over_coverage": 384
    }
  ],
  "ipv6": [],
  "stats": {
    "input_ipv4_count": 500000,
    "output_ipv4_count": 1000,
    "total_ipv4_over_coverage": 28456,
    "ipv4_compression_ratio": 500.0,
    "ipv4_target_binding": true
  }
}
```

### AWS (prefix list entries)
Array suitable for `aws ec2 modify-managed-prefix-list --add-entries`:
```json
[
  {"Cidr": "10.0.0.0/22"},
  {"Cidr": "192.168.0.0/16"},
  {"Cidr": "2001:db8::/32"}
]
```

## 9. Differentiation from Existing Tools

| Feature | rs-aggregate | aggregate6 | cidr-merger | **cidr-list-optimizer** |
|---------|-------------|-----------|------------|------------------------|
| Lossless aggregation | ✓ | ✓ | ✓ | ✓ |
| Lossy optimization to budget | ✗ | ✗ | ✗ | **✓** |
| Provenance tracking | ✗ | ✗ | ✗ | **✓** |
| Over-coverage reporting | ✗ | ✗ | ✗ | **✓** |
| Target entry budget | ✗ | ✗ | ✗ | **✓** |
| Per-AF budgets | ✗ | ✗ | ✗ | **✓** |
| AWS output formats | ✗ | ✗ | ✗ | **✓** |
| Provably optimal lossy | ✗ | ✗ | ✗ | Near-optimal greedy |
| Progress/cancellation | ✗ | ✗ | ✗ | **✓** |

## 10. References

1. Draves, R. et al. "Constructing Optimal IP Routing Tables" (1999) — https://www.microsoft.com/en-us/research/wp-content/uploads/2016/02/tr-98-59.pdf
2. FAQS: Fast Aggregation with Quick Selections (2018) — https://arxiv.org/abs/1812.05520
3. FIFA: Fast Incremental FIB Aggregation (2013) — https://ieeexplore.ieee.org/document/6566913/
4. SMALTA IETF Draft — https://datatracker.ietf.org/doc/html/draft-uzmi-smalta-01
5. RFC 4632: CIDR — https://www.rfc-editor.org/rfc/rfc4632
6. Zhao, J. et al. "On the Aggregatability of Router Forwarding Tables," IEEE/ACM Transactions on Networking, 20(3), 2012. — Proves lossless FIB aggregation is polynomial-time solvable via DP.
7. Johnson, D.S. & Niemi, K.A. "On Knapsacks, Partitions, and a New Dynamic Programming Technique for Trees" (1983) — https://pubsonline.informs.org/doi/pdf/10.1287/moor.8.1.1 — *Provides pseudo-polynomial DP algorithms for tree-structured optimization; a future O(n·k) tree DP could provide provably optimal results.*
8. AWS VPC Quotas — https://docs.aws.amazon.com/vpc/latest/userguide/amazon-vpc-limits.html
9. AWS WAF Quotas — https://docs.aws.amazon.com/waf/latest/developerguide/limits.html
10. AWS Network Firewall Quotas — https://docs.aws.amazon.com/network-firewall/latest/developerguide/quotas.html
