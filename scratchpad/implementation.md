# CIDR List Optimizer — Technical Implementation Plan (v5)

*Revision v5: adversarial review fixes — heap key changed to cost-efficiency (cost/(leaf_count-1)), over-coverage tracking simplified to += net_new_over, operation ordering fixed (invalidate before collapse), ancestor generation bumped on re-insert, ratio check overflow handling fixed symmetrically, false optimality claim removed.*

## 1. Project Structure

```
cidr-list-optimizer/
├── Cargo.toml                    # Workspace root
├── crates/
│   ├── cidr-optimizer/           # Library crate
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs            # Public API, re-exports
│   │       ├── error.rs          # Error types (split: OptimizeError + OptimizerError)
│   │       ├── parser.rs         # Input parsing
│   │       ├── lossless.rs       # Sort-based lossless aggregation
│   │       ├── trie.rs           # Path-compressed binary trie (arena-allocated)
│   │       ├── optimizer.rs      # Greedy collapse with BinaryHeap
│   │       ├── provenance.rs     # Provenance mapping (output → input)
│   │       └── types.rs          # Public types
│   └── cidr-optimizer-cli/       # CLI binary crate
│       ├── Cargo.toml
│       └── src/
│           └── main.rs
├── fuzz/                         # cargo-fuzz targets
│   ├── Cargo.toml
│   └── fuzz_targets/
│       ├── fuzz_parse.rs
│       ├── fuzz_optimize.rs
│       └── fuzz_lossless.rs
├── tests/
│   ├── integration.rs
│   ├── differential.rs           # Differential tests vs ipnet::aggregate()
│   └── fixtures/
└── benches/
    └── optimize.rs
```

## 2. Dependencies

### Library crate (`cidr-optimizer`)
```toml
[dependencies]
ipnet = "2"
thiserror = "2"

[dev-dependencies]
criterion = { version = "0.5", features = ["html_reports"] }
rand = "0.8"
proptest = "1"
```

### CLI crate (`cidr-optimizer-cli`)
```toml
[dependencies]
cidr-optimizer = { path = "../cidr-optimizer" }
clap = { version = "4", features = ["derive"] }
anyhow = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

### Fuzz crate (`fuzz/`)
```toml
[dependencies]
cidr-optimizer = { path = "../crates/cidr-optimizer" }
libfuzzer-sys = "0.4"
arbitrary = { version = "1", features = ["derive"] }
```

## 3. Module Design

### 3.1 `types.rs`

```rust
use ipnet::IpNet;
use std::ops::ControlFlow;

pub struct OptimizerConfig {
    pub ipv4_target: Option<usize>,
    pub ipv6_target: Option<usize>,
    pub max_over_coverage_ratio: Option<f64>,
    pub max_prefix_len_v4: u8,
    pub max_prefix_len_v6: u8,
    pub max_input_entries: usize,
    pub provenance: bool,
}

impl Default for OptimizerConfig {
    fn default() -> Self {
        Self {
            ipv4_target: None,
            ipv6_target: None,
            max_over_coverage_ratio: None,
            max_prefix_len_v4: 32,
            max_prefix_len_v6: 128,
            max_input_entries: 10_000_000,
            provenance: false,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum AddressFamily { IPv4, IPv6 }

/// Progress information passed to callback
pub enum Phase {
    Parsing { entries_read: usize },
    Lossless { af: AddressFamily, entries_remaining: usize },
    Lossy { af: AddressFamily, current_count: usize, target: usize },
    Done,
}

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
    pub ipv4_target_binding: bool,
    pub ipv6_target_binding: bool,
}
```

### 3.2 `error.rs`

```rust
use thiserror::Error;

/// Errors from optimize() / optimize_iter() — no Parse variant
#[derive(Error, Debug)]
pub enum OptimizeError {
    #[error("no valid entries in input")]
    EmptyInput,

    #[error("invalid config: {message}")]
    InvalidConfig { message: String },

    #[error("target {target} too small (minimum: {minimum})")]
    TargetTooSmall { target: usize, minimum: usize },

    #[error("input too large: {count} entries exceeds limit of {limit}")]
    InputTooLarge { count: usize, limit: usize },

    #[error("trie arena overflow: exceeded u32::MAX nodes")]
    ArenaOverflow,

    #[error("optimization cancelled by progress callback")]
    Cancelled,
}

/// Errors from optimize_from_reader() — includes parsing
#[derive(Error, Debug)]
pub enum OptimizerError {
    #[error(transparent)]
    Optimize(#[from] OptimizeError),

    #[error("parse error at line {line}: {message}")]
    Parse { line: usize, message: String },

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
```

### 3.3 `parser.rs`

```rust
pub struct ParsedInput {
    pub ipv4: Vec<(usize, Ipv4Net)>,  // (original_index, normalized prefix)
    pub ipv6: Vec<(usize, Ipv6Net)>,
    pub original_strings: Vec<String>, // stored only if provenance enabled
    pub total_entries: usize,
    pub parse_warnings: Vec<(usize, String)>,
}

pub fn parse_input(
    input: impl BufRead,
    store_strings: bool,
    max_entries: usize,
) -> Result<ParsedInput, OptimizerError>;
```

- Bare IPs → `/32` or `/128`
- Normalize via `trunc()`; emit warning for non-canonical CIDRs (host bits set)
- `store_strings` = false skips storing original strings (saves ~20MB for 1M entries)
- Returns `InputTooLarge` error if entry count exceeds `max_entries`

### 3.4 `lossless.rs` — Sort-Based Lossless Aggregation

```rust
pub struct ProvenancePrefix<N> {
    pub prefix: N,
    pub source_indices: Vec<usize>,
}

pub fn lossless_aggregate_v4(
    input: Vec<(usize, Ipv4Net)>,
    max_prefix_len: u8,
) -> Vec<ProvenancePrefix<Ipv4Net>>;

pub fn lossless_aggregate_v6(
    input: Vec<(usize, Ipv6Net)>,
    max_prefix_len: u8,
) -> Vec<ProvenancePrefix<Ipv6Net>>;
```

#### Algorithm

```
1. Radix sort by (network_address, prefix_length) — O(n) for fixed-width keys.
   For IPv4: 5 passes of radix-256 (4 address bytes + 1 prefix_len byte).
   For IPv6: 17 passes of radix-256 (16 address bytes + 1 prefix_len byte), LSD order:
     - Passes 1-16: address bytes from least significant (byte 15) to most significant (byte 0)
     - Pass 17: prefix_len byte
   Alternative: 9 passes of radix-65536 (2 bytes per pass) for ~2× fewer scatters
   at the cost of 512KB histogram per pass (still fits in L2 cache).

2. Redundancy elimination (monotone stack with pop-loop):
   for each prefix P in sorted order:
       while stack not empty AND stack.top() does NOT contain P:
           pop() → emit to output
       if stack not empty AND stack.top() contains P:
           P is redundant → record provenance at stack.top()
       else:
           push P onto stack
   Drain remaining stack entries to output.

3. Enforce max_prefix_len:
   - Truncate any prefix with length > max_prefix_len
   - Re-sort (radix) and re-deduplicate (truncation creates duplicates)
   - Merge provenance of duplicates

4. Stack-based sibling merging (bottom-up with cascading):
   - Sort output by (prefix_length DESC, network_address ASC) via radix sort
   - Use a stack-based approach:
     - For each entry in sorted order, push onto stack
     - After each push, loop: if top two stack entries are siblings
       (same prefix_length, network addresses differ only in bit at position L-1),
       pop both, merge to parent (length L-1, union source_indices), push merged
       result back onto stack. Repeat until top two are NOT siblings or stack has <2 entries.
     - This naturally handles cascading: merging /24→/23 may create a sibling pair
       at /23 with the new stack top, triggering further merges up the tree.
   - After all entries processed, drain stack to output.
   - O(n) amortized — each entry is pushed once; total merges ≤ n-1 (each merge
     reduces entry count by 1). Stack depth bounded by address bit width.
```

### 3.5 `trie.rs` — Path-Compressed Binary Trie (For Lossy Phase Only)

Built from lossless output (typically 100K-500K entries).

```rust
type NodeIdx = u32;
const INVALID: NodeIdx = u32::MAX;

// Field order chosen to minimize padding (u128 fields first for alignment):
#[repr(C)]
struct TrieNode {
    skip_bits: u128,           // 16 bytes (compressed bit pattern) — offset 0
    coverage: u128,            // 16 bytes                         — offset 16
    collapsed_cost_sum: u128,  // 16 bytes (sum of collapse costs of descendant collapsed leaves) — offset 32
    children: [NodeIdx; 2],    //  8 bytes                         — offset 48
    parent: NodeIdx,           //  4 bytes                         — offset 56
    leaf_count: u32,           //  4 bytes                         — offset 60
    generation: u32,           //  4 bytes                         — offset 64
    depth: u8,                 //  1 byte                          — offset 68
    is_leaf: bool,             //  1 byte                          — offset 69
    skip_len: u8,              //  1 byte                          — offset 70
    _pad: u8,                  //  1 byte                          — offset 71
}
// 72 bytes data, struct alignment = 16 (from u128), size rounds to 80 bytes.
// Use const_assert to verify at compile time:
const _: () = assert!(std::mem::size_of::<TrieNode>() == 80);

pub struct BinaryTrie {
    arena: Vec<TrieNode>,
    root: NodeIdx,
    addr_bits: u8,  // 32 or 128
}
```

#### Path Compression

Single-child chains are compressed into a single node with `skip_len > 0`. This is critical for IPv6 /128 entries where paths can be 128 bits deep.

**Formal invariants** (enforced via `debug_assert` on every trie mutation):
```rust
// INV-1: depth + skip_len = prefix_length of the range this node represents
//         For a leaf at /24: depth + skip_len = 24
// INV-2: depth = parent.depth + parent.skip_len + 1  (for non-root nodes)
//         The +1 accounts for the branch bit at the parent
// INV-3: skip_len <= addr_bits - depth  (cannot extend past address width)
// INV-4: For root: depth = 0, skip_len = 0 (root represents ::/0 or 0.0.0.0/0)
// INV-5: skip_bits stores only the LOWER skip_len bits (upper bits must be 0)
//         i.e., skip_bits < (1u128 << skip_len)  [when skip_len < 128]
// INV-6: capacity(node) = 2^(addr_bits - (depth + skip_len))
//         = number of IPs in the prefix range this node represents
//         When addr_bits - (depth + skip_len) >= 128, returns u128::MAX (saturating)
// SAFETY: All arithmetic on depth + skip_len MUST cast to u16 first to prevent
//         silent u8 overflow in release builds (max valid sum is 128 for IPv6).
```

- **`depth` semantics**: The bit position in the address where this node's compressed segment BEGINS. For a node reached by branching left/right at its parent, `depth` = parent's prefix_len + 1 (the bit after the parent's branch point). The node "owns" bits `[depth, depth + skip_len)` of the address.
- **`skip_bits` safety**: When extracting bit `i` (0-indexed from MSB of the compressed segment), use `(skip_bits >> (skip_len - 1 - i)) & 1` where `i < skip_len`. Guard with `debug_assert!(i < skip_len && skip_len <= 128)`.
- **Insert**: When traversing, if a skip doesn't match the key, split the compressed node into two nodes (branch point + remainder). The split preserves INV-1 and INV-2.
- **Collapse**: When a node is collapsed, its compressed descendants are absorbed. No decompression needed — the collapsed node simply becomes a leaf.
- **Memory savings**: For 100K scattered /128 entries, reduces from ~12.8M nodes to ~200K-400K nodes.
- **Path compression safety for greedy**: Compressed (single-child) nodes have `leaf_count = 1` and are never collapse candidates (`leaf_count >= 2` required). Therefore path compression does not remove any valid candidate from the greedy's consideration set.

#### IPv6 Root Node — Overflow Handling

The root node (::/0) has capacity 2^128 which cannot be represented in u128 (max = 2^128 − 1). Handle via:
- Root node (depth == 0) gets capacity `u128::MAX` (= 2^128 − 1, off by 1)
- **Impact of off-by-one**: cost(root) = capacity - coverage = (2^128−1) − coverage. This underestimates the true cost by exactly 1 IP out of 2^128 — a relative error of 2^−128, completely negligible for any practical ratio check.
- Root gets efficiency `u128::MAX` (saturating) — collapsed only as last resort in min-heap
- This allows target=1 to work (root IS a valid collapse candidate, just maximally expensive)
- If input contains `::/0`, root coverage = `u128::MAX` (saturating), cost = 0 (correct: full coverage means zero over-coverage regardless of the off-by-one)

```rust
fn capacity(&self, node_idx: NodeIdx) -> u128 {
    let node = &self.arena[node_idx as usize];
    if node.depth == 0 && node.skip_len == 0 {
        // True capacity is 2^128, but u128::MAX = 2^128-1.
        // Off-by-one (1 part in 2^128) is negligible for all practical purposes.
        return u128::MAX;
    }
    // For non-root nodes: depth + skip_len gives the prefix length of this node's range.
    // capacity = 2^(addr_bits - prefix_len)
    // Cast to u16 to prevent silent u8 overflow in release builds.
    let prefix_len = node.depth as u16 + node.skip_len as u16;
    debug_assert!(prefix_len <= self.addr_bits as u16);
    if prefix_len == self.addr_bits as u16 { return 1; } // single host
    let shift = self.addr_bits as u16 - prefix_len;
    // Guard: shift >= 128 means capacity >= 2^128, use saturating u128::MAX
    // (only reachable for IPv6 root-adjacent nodes with prefix_len=0, handled above)
    if shift >= 128 { return u128::MAX; }
    1u128 << shift
}

fn collapse_cost(&self, node_idx: NodeIdx) -> u128 {
    self.capacity(node_idx).saturating_sub(self.arena[node_idx as usize].coverage)
}

/// Collapse a node into a leaf, absorbing its entire subtree.
///
/// **Contract** (violating any of these is a logic bug):
/// - Sets `is_leaf = true`
/// - Sets `leaf_count = 1`
/// - Sets `children = [INVALID, INVALID]`
/// - Does NOT modify `parent`, `depth`, `skip_len`, `coverage`, or `skip_bits`
fn collapse(&mut self, node_idx: NodeIdx) {
    let node = &mut self.arena[node_idx as usize];
    node.is_leaf = true;
    node.leaf_count = 1;
    node.children = [INVALID, INVALID];
}
```

#### Arena Overflow Guard

```rust
fn alloc_node(&mut self) -> Result<NodeIdx, OptimizeError> {
    let len = self.arena.len();
    // Use try_into to prevent silent truncation at u32 boundary
    let idx: NodeIdx = len.try_into().map_err(|_| OptimizeError::ArenaOverflow)?;
    if idx == INVALID {
        return Err(OptimizeError::ArenaOverflow); // reserve INVALID sentinel
    }
    self.arena.push(TrieNode::default());
    Ok(idx)
}
```

### 3.6 `optimizer.rs` — Greedy Collapse with BinaryHeap (Cost-Efficiency)

```rust
use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// Heap key: (cost, leaf_count_minus_1, node_idx, generation)
/// Comparison uses widening 160-bit multiplication for exact transitive ordering:
///   a is cheaper than b iff a.cost * b.savings < b.cost * a.savings
/// where savings = leaf_count - 1. Since cost is u128 and savings is u32,
/// the product fits in 160 bits — computed as (high_u32, low_u128) pairs.
/// Stored as (cost, savings, node_idx, generation) for the Ord-based comparison.
/// Ties broken by node_idx for determinism.
type HeapEntry = Reverse<(EfficiencyKey, u32, u32)>;

/// Efficiency key that compares via widening multiplication to avoid overflow.
/// Represents the fraction cost/savings where savings = leaf_count - 1.
/// Since cost is u128 and savings is u32, the cross-product fits in 160 bits.
/// We use (high_u32, low_u128) pairs for exact, transitive comparison.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct EfficiencyKey {
    cost: u128,
    savings: u32,  // leaf_count - 1, always >= 1
}

impl Ord for EfficiencyKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Compare cost/savings vs other.cost/other.savings
        // Without division: self.cost * other.savings <=> other.cost * self.savings
        // Since cost is u128 and savings is u32, the product requires at most 160 bits.
        // We use widened multiplication to (high_u32, low_u128) pairs for exact comparison.
        let lhs = widening_mul_u128_u32(self.cost, other.savings);
        let rhs = widening_mul_u128_u32(other.cost, self.savings);
        lhs.cmp(&rhs)
    }
}

/// Multiply a u128 by a u32, returning a (high_u32, low_u128) pair.
/// The result represents high * 2^128 + low (at most 160 bits, never overflows).
#[inline]
fn widening_mul_u128_u32(a: u128, b: u32) -> (u32, u128) {
    // Split a into two u64 halves: a = a_hi * 2^64 + a_lo
    let a_lo = a as u64 as u128;   // lower 64 bits
    let a_hi = a >> 64;            // upper 64 bits
    let b = b as u128;

    // Partial products (each fits in u128: max 64+32 = 96 bits)
    let prod_lo = a_lo * b;        // contributes to bits [0..96]
    let prod_hi = a_hi * b;        // contributes to bits [64..160]

    // Combine: result = prod_hi * 2^64 + prod_lo
    let (low, carry) = prod_lo.overflowing_add(prod_hi << 64);
    let high = (prod_hi >> 64) as u32 + carry as u32;
    (high, low)
}

impl PartialOrd for EfficiencyKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

fn efficiency_key(cost: u128, leaf_count: u32) -> EfficiencyKey {
    debug_assert!(leaf_count >= 2, "efficiency_key called with leaf_count={}", leaf_count);
    EfficiencyKey { cost, savings: leaf_count - 1 }
}

pub fn optimize_trie(
    trie: &mut BinaryTrie,
    target: usize,
    max_ratio: Option<f64>,
    input_covered_ips: u128,
) -> Vec<NodeIdx> {
    let mut remaining = trie.total_leaf_count();
    if remaining <= target {
        return trie.extract_leaf_indices();
    }

    let mut heap: BinaryHeap<HeapEntry> = BinaryHeap::new();

    // Track actual over-coverage of current leaf set
    let mut current_over_coverage: u128 = 0;

    // Initialize: all internal nodes with leaf_count >= 2
    for idx in 0..trie.arena.len() {
        let node = &trie.arena[idx];
        if node.leaf_count >= 2 && !node.is_leaf {
            let cost = trie.collapse_cost(idx as u32);
            let eff = efficiency_key(cost, node.leaf_count);
            heap.push(Reverse((eff, idx as u32, node.generation)));
        }
    }

    while remaining > target {
        let Some(Reverse((_eff, node_idx, gen))) = heap.pop() else { break };

        // O(1) staleness check — must re-read node from arena
        let node = &trie.arena[node_idx as usize];
        if node.is_leaf || node.generation != gen {
            // Periodic heap compaction: when stale entries dominate (>75% of heap),
            // rebuild from scratch to reclaim memory. Avoids dependency on
            // BinaryHeap::retain (stabilized only in Rust 1.70).
            // Worst-case heap size without compaction is
            // O(collapses × depth) ≈ 500K × 128 = 64M entries (1.5 GB for IPv6).
            // Compaction bounds effective memory to ~4× live entries.
            if heap.len() > 4 * remaining {
                let arena = &trie.arena;
                heap = BinaryHeap::from(
                    heap.into_vec()
                        .into_iter()
                        .filter(|Reverse((_, idx, g))| {
                            let n = &arena[*idx as usize];
                            !n.is_leaf && n.generation == *g
                        })
                        .collect::<Vec<_>>()
                );
            }
            continue;
        }

        let leaf_count = node.leaf_count;
        debug_assert!(leaf_count >= 2, "non-leaf node {} has leaf_count={}, expected >= 2", node_idx, leaf_count);
        let reduction = leaf_count as usize - 1;
        if reduction == 0 { continue; }

        // Recompute cost (efficiency may be stale if leaf_count changed)
        let cost = trie.collapse_cost(node_idx);

        // Compute net over-coverage change:
        // collapsed_cost_sum is maintained incrementally — O(1) lookup.
        // It tracks the sum of collapse costs of all currently-collapsed descendant leaves.
        let descendant_collapsed_cost = trie.arena[node_idx as usize].collapsed_cost_sum;
        // Invariant: cost(P) >= sum of descendant collapsed costs, because cost is additive
        // and descendants' costs are subsets of P's cost. Violation indicates a bookkeeping bug.
        debug_assert!(
            cost >= descendant_collapsed_cost,
            "over-coverage invariant violated: cost({}) = {} < descendant_collapsed_cost = {}",
            node_idx, cost, descendant_collapsed_cost
        );
        let net_new_over = cost.saturating_sub(descendant_collapsed_cost);

        // Check max_over_coverage_ratio BEFORE collapsing
        if let Some(max_ratio) = max_ratio {
            if input_covered_ips > 0 {
                let new_total = current_over_coverage.saturating_add(net_new_over);
                if exceeds_ratio(new_total, input_covered_ips, max_ratio) {
                    break;
                }
            }
        }

        // Update over-coverage with net change
        current_over_coverage = current_over_coverage.saturating_add(net_new_over);

        // IMPORTANT: invalidate subtree BEFORE collapse to ensure any concurrent
        // heap entries for descendants are marked stale before structural changes
        trie.invalidate_subtree(node_idx);

        // Execute collapse — contract: collapse() MUST set:
        //   node.is_leaf = true
        //   node.leaf_count = 1
        //   node.children = [INVALID, INVALID]
        trie.collapse(node_idx);

        // Safe subtraction: clamp to 0 (should never underflow if logic is correct)
        remaining = remaining.saturating_sub(reduction);

        // Update ancestors: recompute leaf_counts, bump generation, push new candidates,
        // and propagate collapsed_cost_sum incrementally.
        // When node P is collapsed with cost C, ancestors gain C but lose the
        // descendant_collapsed_cost that P previously contributed (those are now absorbed).
        // Net delta to each ancestor's collapsed_cost_sum = cost - descendant_collapsed_cost = net_new_over.
        let mut ancestor = trie.arena[node_idx as usize].parent;
        while ancestor != INVALID {
            trie.update_leaf_count(ancestor);
            trie.arena[ancestor as usize].collapsed_cost_sum =
                trie.arena[ancestor as usize].collapsed_cost_sum
                    .saturating_add(net_new_over);
            // Bump generation so old heap entries for this ancestor become stale
            trie.arena[ancestor as usize].generation = trie.arena[ancestor as usize].generation.wrapping_add(1);
            let a = &trie.arena[ancestor as usize];
            if !a.is_leaf && a.leaf_count >= 2 {
                let anc_cost = trie.collapse_cost(ancestor);
                let anc_eff = efficiency_key(anc_cost, a.leaf_count);
                heap.push(Reverse((anc_eff, ancestor, a.generation)));
            }
            ancestor = trie.arena[ancestor as usize].parent;
        }
    }

    trie.extract_leaf_indices()
}

/// Integer-scaled ratio check to avoid f64 precision loss for large IPv6 values.
///
/// We want to check: over / covered > max_ratio
/// Rearranged to avoid division: over > max_ratio × covered
/// Scaled to integers: over × SCALE > (max_ratio × SCALE) × covered
///
/// # Overflow Safety Analysis
///
/// **Branch 1 (f64 fast path)**: When both values ≤ u64::MAX (≈ 1.8×10^19), f64 has
/// 53 bits of mantissa. Values up to 2^53 are exact; values in [2^53, 2^64] have
/// relative error ≤ 2^-53 ≈ 10^-16. For a ratio comparison this is negligible.
/// All IPv4 values (max 2^32) are exact in f64.
///
/// **Branch 2a (RHS overflow)**: threshold × covered overflows u128. Since threshold =
/// max_ratio × SCALE ≤ 1×10^6, overflow requires covered ≥ 2^128/10^6 ≈ 3.4×10^32.
/// Meanwhile scaled_over = over × 10^6 didn't overflow, so over < 2^128/10^6.
/// Therefore over < covered, meaning over/covered < 1 ≤ max_ratio is impossible
/// (max_ratio ∈ [0,1]). Actually: over/covered < 1.0, and since max_ratio ≥ 0,
/// the ratio is NOT exceeded. Returning `false` is **correct and conservative**.
///
/// **Branch 2b (LHS overflow)**: over × SCALE overflows u128, meaning over ≥ 2^128/10^6
/// ≈ 3.4×10^32. This is an astronomically large over-coverage. The f64 fallback has
/// relative precision ~2^-53 which is adequate for a ratio comparison (we don't need
/// exact boundary behavior for values this large — any reasonable ratio would be exceeded).
fn exceeds_ratio(over: u128, covered: u128, max_ratio: f64) -> bool {
    // Branch 1: f64 fast path — exact for IPv4, negligible error for values ≤ u64::MAX
    if over <= u64::MAX as u128 && covered <= u64::MAX as u128 {
        return (over as f64) > max_ratio * (covered as f64);
    }
    // Branch 2: integer scaling for large IPv6 values
    const SCALE: u128 = 1_000_000; // 6 decimal places of ratio precision
    let threshold = (max_ratio * SCALE as f64) as u128;
    match over.checked_mul(SCALE) {
        Some(scaled_over) => {
            match threshold.checked_mul(covered) {
                Some(rhs) => scaled_over > rhs,
                // Branch 2a: RHS overflow → covered is enormous relative to threshold,
                // so over/covered must be tiny. Ratio NOT exceeded. (See safety proof above.)
                None => false,
            }
        }
        // Branch 2b: LHS overflow → over is enormous (≥ 3.4×10^32).
        // Fall back to f64 which has adequate relative precision for this magnitude.
        None => (over as f64 / covered as f64) > max_ratio,
    }
}
```

#### Why BinaryHeap with Cost-Efficiency Key

- **Cost-efficiency ranking**: Key is `cost / (leaf_count - 1)` compared via widening multiplication (`a.cost * b.savings <=> b.cost * a.savings` using 160-bit `(u32, u128)` pairs). This prevents bias toward tiny 2-leaf merges (low absolute cost but poor efficiency). A node saving 3 entries at cost 6 (eff=2) is preferred over a node saving 1 entry at cost 3 (eff=3). Widening arithmetic guarantees exact, transitive ordering with no overflow or precision loss.
- **Practical performance**: For k ≤ 500K candidates, O(log k) ≈ O(19) per operation. BinaryHeap is faster than bucket queues due to cache-contiguous storage.
- **Structural determinism**: Composite key `(efficiency, node_idx, generation)` guarantees deterministic extraction order. No implementation-dependent tie-breaking.
- **Simplicity**: Zero custom data structure code. `std::collections::BinaryHeap` is battle-tested.
- **Staleness via generation**: Ancestors get generation bumped (wrapping on u32 overflow) on re-insert, ensuring old heap entries are discarded. No double-processing. Wrapping is safe because staleness detection uses `!=` comparison: a stale entry's stored generation will differ from the current generation regardless of wrap-around, except in the astronomically unlikely case of exactly 2^32 re-insertions of the same node between a heap push and its pop (would require >4 billion collapses, far exceeding any practical input).

#### Over-Coverage Tracking (Correct)

The `current_over_coverage` tracks the actual over-coverage of the current leaf set:
- Original lossless leaves have cost 0 (no over-coverage)
- When node P is collapsed, it becomes a leaf with cost(P)
- If P's subtree contained previously-collapsed leaves (with their own costs), those are removed from the sum and P's cost replaces them
- **Net change per collapse**: `+net_new_over` where `net_new_over = cost(P) - sum_of_collapsed_descendant_costs`
- This is computed ONCE and applied directly via `current_over_coverage += net_new_over`
- No subtract-then-add pattern (which would mask errors via saturating arithmetic)

#### Operation Ordering (Critical)

```
1. Compute net_new_over and check ratio (BEFORE any mutation)
2. Update current_over_coverage
3. Invalidate subtree (marks descendant heap entries stale)
4. Collapse node (structural mutation — detach children, mark as leaf)
5. Update ancestors (bump generation, recompute leaf_count, push new entries)
```

Invalidating BEFORE collapse ensures that if any descendant heap entries are popped between steps, they fail the staleness check. Bumping ancestor generation on re-insert ensures old ancestor entries are discarded.

### 3.7 `provenance.rs`

```rust
/// Map output prefixes back to original input indices via binary search.
pub fn compute_provenance_v4(
    output_prefixes: &[Ipv4Net],
    original_input: &[(usize, Ipv4Net)],  // sorted by network addr
) -> Vec<Vec<usize>> {
    output_prefixes.iter().map(|out| {
        let start = original_input.partition_point(|(_, p)| p.network() < out.network());
        let end = original_input.partition_point(|(_, p)| p.network() <= out.broadcast());
        original_input[start..end]
            .iter()
            .filter(|(_, p)| out.contains(p))
            .map(|(idx, _)| *idx)
            .collect()
    }).collect()
}
```

### 3.8 `lib.rs` — Public API

```rust
use std::ops::ControlFlow;

/// Primary API: optimize from pre-parsed prefixes.
pub fn optimize(
    prefixes: &[IpNet],
    config: &OptimizerConfig,
) -> Result<OptimizationResult, OptimizeError> {
    optimize_with_progress(prefixes, config, |_| ControlFlow::Continue(()))
}

/// Iterator-based API: accepts any IntoIterator, partitions and collects internally.
/// Avoids requiring the caller to build a Vec<IpNet>, but still allocates O(n) internally
/// for partitioned per-AF vectors (required by radix sort and trie construction).
pub fn optimize_iter(
    prefixes: impl IntoIterator<Item = IpNet>,
    config: &OptimizerConfig,
) -> Result<OptimizationResult, OptimizeError> {
    let (ipv4, ipv6) = partition_iter_with_indices(prefixes, config.max_input_entries)?;
    optimize_partitioned(ipv4, ipv6, config, |_| ControlFlow::Continue(()))
}

/// With progress callback and cancellation support.
pub fn optimize_with_progress(
    prefixes: &[IpNet],
    config: &OptimizerConfig,
    mut progress: impl FnMut(Phase) -> ControlFlow<()>,
) -> Result<OptimizationResult, OptimizeError> {
    if prefixes.len() > config.max_input_entries {
        return Err(OptimizeError::InputTooLarge {
            count: prefixes.len(),
            limit: config.max_input_entries,
        });
    }

    // Config validation
    if config.max_prefix_len_v4 > 32 {
        return Err(OptimizeError::InvalidConfig {
            message: format!("max_prefix_len_v4 ({}) exceeds 32", config.max_prefix_len_v4),
        });
    }
    if config.max_prefix_len_v6 > 128 {
        return Err(OptimizeError::InvalidConfig {
            message: format!("max_prefix_len_v6 ({}) exceeds 128", config.max_prefix_len_v6),
        });
    }
    if let Some(r) = config.max_over_coverage_ratio {
        if r < 0.0 || r > 1.0 || r.is_nan() {
            return Err(OptimizeError::InvalidConfig {
                message: format!("max_over_coverage_ratio ({}) must be in [0.0, 1.0]", r),
            });
        }
    }

    let (ipv4, ipv6) = partition_with_indices(prefixes);

    if ipv4.is_empty() && ipv6.is_empty() {
        return Err(OptimizeError::EmptyInput);
    }

    if let Some(t) = config.ipv4_target {
        if t == 0 && !ipv4.is_empty() {
            return Err(OptimizeError::TargetTooSmall { target: 0, minimum: 1 });
        }
    }
    if let Some(t) = config.ipv6_target {
        if t == 0 && !ipv6.is_empty() {
            return Err(OptimizeError::TargetTooSmall { target: 0, minimum: 1 });
        }
    }

    progress(Phase::Lossless { af: AddressFamily::IPv4, entries_remaining: ipv4.len() });
    let lossless_v4 = lossless::lossless_aggregate_v4(ipv4, config.max_prefix_len_v4);

    progress(Phase::Lossless { af: AddressFamily::IPv6, entries_remaining: ipv6.len() });
    let lossless_v6 = lossless::lossless_aggregate_v6(ipv6, config.max_prefix_len_v6);

    let ipv4_target_binding = config.ipv4_target.map_or(false, |t| lossless_v4.len() > t);
    let ipv6_target_binding = config.ipv6_target.map_or(false, |t| lossless_v6.len() > t);

    let output_v4 = match config.ipv4_target {
        Some(target) if lossless_v4.len() > target => {
            let input_ips = compute_covered_ips_v4(&lossless_v4);
            progress(Phase::Lossy {
                af: AddressFamily::IPv4,
                current_count: lossless_v4.len(),
                target,
            });
            lossy_optimize_v4(&lossless_v4, target, config.max_over_coverage_ratio, input_ips)?
        }
        _ => lossless_v4,
    };

    let output_v6 = match config.ipv6_target {
        Some(target) if lossless_v6.len() > target => {
            let input_ips = compute_covered_ips_v6(&lossless_v6);
            progress(Phase::Lossy {
                af: AddressFamily::IPv6,
                current_count: lossless_v6.len(),
                target,
            });
            lossy_optimize_v6(&lossless_v6, target, config.max_over_coverage_ratio, input_ips)?
        }
        _ => lossless_v6,
    };

    progress(Phase::Done);
    build_result(output_v4, output_v6, prefixes, config, ipv4_target_binding, ipv6_target_binding)
}

/// Convenience: parse from reader then optimize.
pub fn optimize_from_reader(
    input: impl BufRead,
    config: &OptimizerConfig,
) -> Result<OptimizationResult, OptimizerError> {
    let parsed = parser::parse_input(input, config.provenance, config.max_input_entries)?;
    let prefixes: Vec<IpNet> = reconstruct_from_parsed(&parsed);
    Ok(optimize(&prefixes, config)?)
}
```

#### Over-Coverage Calculation

```rust
// CORRECT: use 2^(bits - prefix_len) for total IPs in a prefix
fn prefix_ip_count(prefix_len: u8, addr_bits: u8) -> u128 {
    if addr_bits == prefix_len { return 1; }
    1u128 << (addr_bits - prefix_len)
}

// NOT hosts().count() which excludes network/broadcast addresses
```

#### Output Validation (Optional)

```rust
/// Verify that every input prefix is contained by at least one output prefix.
/// Called when --validate flag is passed.
pub fn validate_coverage(input: &[IpNet], output: &[AggregatedEntry]) -> bool {
    let output_sorted: Vec<_> = output.iter().map(|e| &e.prefix).collect();
    input.iter().all(|inp| {
        output_sorted.iter().any(|out| out.contains(inp))
    })
}
```

## 4. CLI Implementation

```rust
#[derive(Parser)]
#[command(name = "cidr-optimizer")]
#[command(about = "Optimize IP/CIDR lists to fit per-AF entry budgets")]
struct Cli {
    /// Input file (- for stdin)
    #[arg(default_value = "-")]
    input: String,

    /// IPv4 target entry count (omit for lossless)
    #[arg(long)]
    ipv4_target: Option<usize>,

    /// IPv6 target entry count (omit for lossless)
    #[arg(long)]
    ipv6_target: Option<usize>,

    /// Maximum over-coverage ratio per AF (over_coverage / input_ips)
    #[arg(long)]
    max_over_coverage: Option<f64>,

    /// Maximum output prefix length for IPv4
    #[arg(long, default_value = "32")]
    max_prefix_len_v4: u8,

    /// Maximum output prefix length for IPv6
    #[arg(long, default_value = "128")]
    max_prefix_len_v6: u8,

    /// Maximum input entries (default: 10M)
    #[arg(long, default_value = "10000000")]
    max_input_entries: usize,

    /// Output format
    #[arg(long, value_enum, default_value = "plain")]
    format: OutputFormat,

    /// Show provenance (which inputs map to each output)
    #[arg(long)]
    provenance: bool,

    /// Show statistics on stderr
    #[arg(long)]
    stats: bool,

    /// Validate output covers all inputs
    #[arg(long)]
    validate: bool,
}

#[derive(Clone, ValueEnum)]
enum OutputFormat {
    Plain,
    Json,
    Aws,
}
```

## 5. Implementation Phases

### Phase 1: Foundation
- [ ] Workspace setup (including fuzz/ directory)
- [ ] `types.rs`, `error.rs` (split error types)
- [ ] `parser.rs` with unit tests + max_input_entries guard
- [ ] Basic CLI skeleton

### Phase 2: Lossless Aggregation
- [ ] `lossless.rs`: radix sort (5 passes for IPv4, 17 passes for IPv6)
- [ ] Monotone-stack redundancy elimination (with pop-loop)
- [ ] Sibling merging (single-pass with cascading)
- [ ] `max_prefix_len` enforcement
- [ ] Provenance tracking through merges
- [ ] **Differential tests**: verify output matches `ipnet::aggregate()` for lossless mode
- [ ] CLI lossless mode end-to-end

### Phase 3: Path-Compressed Trie + Greedy Optimizer
- [ ] `trie.rs`: path-compressed arena trie, insert with split, build_metadata
- [ ] Arena overflow guard (u32::MAX check)
- [ ] IPv6 root node saturating arithmetic (collapsed only as last resort)
- [ ] `optimizer.rs`: greedy collapse with `BinaryHeap<Reverse<(u128, u32, u32)>>` — cost-efficiency key
- [ ] Cost-efficiency function: `cost / (leaf_count - 1)`
- [ ] Correct over-coverage tracking (`+= net_new_over`, no subtract-then-add)
- [ ] Operation ordering: invalidate → collapse → update ancestors (with generation bump)
- [ ] Integer-scaled ratio check with symmetric overflow handling
- [ ] Deterministic tiebreaking via composite key `(efficiency, node_idx, generation)`
- [ ] Unit tests: collapse correctness, target reached, sparse inputs, ratio cap, generation staleness

### Phase 4: Provenance + Output
- [ ] `provenance.rs`: binary-search mapping
- [ ] `lib.rs`: full pipeline with progress callback
- [ ] `optimize_iter()` API
- [ ] Over-coverage calculation (using `1 << (bits - prefix_len)`)
- [ ] `--validate` flag (coverage invariant check)
- [ ] JSON and AWS output formats (with `target_binding` in stats)
- [ ] Integration tests

### Phase 5: Fuzzing + Polish
- [ ] `fuzz_parse.rs`: arbitrary bytes → parser (catches panics on malformed input)
- [ ] `fuzz_optimize.rs`: arbitrary valid prefix lists + configs → full pipeline
- [ ] `fuzz_lossless.rs`: arbitrary prefixes → lossless phase
- [ ] Property tests: no false negatives, target met, determinism, greedy ≤ brute-force (small inputs)
- [ ] **Differential test**: `lossless_aggregate(input) == ipnet::aggregate(input)` for all inputs
- [ ] Adversarial inputs: worst-case trie shapes, identical costs, maximum cascade
- [ ] Criterion benchmarks (1K–10M entries)
- [ ] Edge cases: empty, single entry, target=1, ::/0, overlapping inputs, all-identical
- [ ] Rustdoc + README

## 6. Key Design Decisions

### 6.1 Hybrid Architecture (Sort + Path-Compressed Trie)
Radix-sort-based lossless handles millions of raw entries in O(n) time and memory. Path-compressed trie built only from lossless output (10-100× smaller). Meets 4GB NFR even for scattered IPv6 /128 inputs at 10M scale.

### 6.2 Path-Compressed Trie Node (80 bytes)
Uniform `u128` coverage for both AFs. `skip_len` + `skip_bits` for path compression. Generation counter for O(1) staleness. `collapsed_cost_sum` for O(1) over-coverage tracking during greedy collapse. No SmallVec, no per-node provenance.

### 6.3 BinaryHeap with Cost-Efficiency Key
`std::collections::BinaryHeap` with composite key `(efficiency, node_idx, generation)`:
- efficiency = `cost / (leaf_count - 1)` — cost per entry saved
- O(log k) per operation — faster in practice than O(128) bucket scan for k ≤ 500K
- Deterministic tie-breaking (structural guarantee, not implementation accident)
- Zero custom data structure code
- Cache-friendly contiguous storage
- Ancestor generation bumped on re-insert to prevent double-processing

### 6.4 Generalized Collapse
Any internal node with `leaf_count >= 2` (except root at depth 0) is a valid candidate. Handles sparse inputs, missing siblings, multi-level collapses.

### 6.5 Greedy Heuristic with Cost-Efficiency Ranking
Cost-additivity guarantees path-independence (total over-coverage depends only on final leaf set). Cost-efficiency ranking (`cost / (leaf_count - 1)`) ensures each unit of entry-budget is spent on the cheapest available over-coverage. The algorithm is a heuristic (general tree knapsack is NP-hard per Johnson & Niemi 1983), but empirically near-optimal for typical blocklist inputs. Property tests verify greedy ≤ 1.05× brute-force for small inputs. An exact O(n×k) tree DP alternative exists (see specification §4.6) but is not implemented in v1 due to higher constant factors and implementation complexity.

### 6.6 External Provenance (Opt-In)
Binary search on sorted input at extraction time. Zero cost during optimization hot loop. `source_indices` is `Option<Vec<usize>>` — None when provenance disabled (no empty Vec allocation).

### 6.7 Per-AF Independent Optimization
Each address family has its own target and is optimized independently. No shared budget complexity. IPv4 and IPv6 tries are separate — can be processed in parallel with rayon in future.

### 6.8 Determinism
`BinaryHeap<Reverse<(u128, u32, u32)>>` — composite key `(efficiency, node_idx, generation)` ensures deterministic extraction. Node indices are deterministic (sorted insertion order). Ancestor generation bumped on re-insert prevents non-deterministic double-processing. Output sorted by network address.

### 6.9 IPv6 Overflow — Saturating Arithmetic
- `capacity(root)` returns `u128::MAX` (approximation of 2^128)
- `collapse_cost()` uses `saturating_sub`
- Root gets cost `u128::MAX` — valid collapse candidate but maximally expensive (last resort for target=1)
- If input contains `::/0`, coverage = `u128::MAX`, cost = 0

### 6.10 Correct Over-Coverage Tracking
Running `current_over_coverage` tracks the true over-coverage of the current leaf set. When collapsing P after a descendant C was already collapsed:
- C is no longer a leaf; P becomes the new leaf
- Net change = `cost(P) - sum_of_collapsed_descendant_costs` (computed as `net_new_over`)
- Applied as single addition: `current_over_coverage += net_new_over`
- No subtract-then-add pattern (saturating_sub would mask bugs by silently clamping to 0)

### 6.11 Integer-Scaled Ratio Check (Overflow-Safe)
For IPv6 ranges larger than /48, f64 loses precision. The ratio check uses:
```
over × SCALE > threshold × covered
```
where SCALE = 1,000,000 and threshold = (max_ratio × SCALE) as u128. Both sides use `checked_mul`:
- If `over × SCALE` overflows → over is enormous, ratio almost certainly exceeded (fall back to f64)
- If `threshold × covered` overflows → covered is enormous relative to threshold, ratio NOT exceeded (return false)
- For IPv4 (values fit in u64), uses direct f64 comparison (exact for u64 range)

### 6.12 Memory Safety
- `max_input_entries` config prevents OOM from unbounded input
- Arena overflow check prevents u32 index wraparound
- Path compression prevents unbounded trie depth for IPv6 /128 entries

## 7. Testing Strategy

### Critical Tests
```rust
#[test] fn sparse_input_reaches_target_1()     // scattered /32s → 0.0.0.0/0
#[test] fn overlapping_provenance_preserved()   // /8 + /16 + /32 → single entry, 3 sources
#[test] fn max_prefix_len_enforced()            // /32 inputs with max=24 → /24 outputs
#[test] fn deterministic_output()               // same input → same output always
#[test] fn ipv6_no_overflow()                   // ::/0 input doesn't panic
#[test] fn ipv6_128_path_compression()          // 100K random /128s doesn't OOM
#[test] fn lossless_removes_redundant()         // /8 subsumes /16, /24
#[test] fn over_coverage_correct()              // uses 2^(32-len), not hosts().count()
#[test] fn over_coverage_no_double_count()      // parent collapse after child: correct total
#[test] fn per_af_independent()                 // ipv4_target doesn't affect ipv6 output
#[test] fn ratio_cap_stops_early()              // max_over_coverage_ratio halts before target
#[test] fn ratio_cap_ipv6_precision()           // large IPv6 ranges: integer check is accurate
#[test] fn no_target_means_lossless()           // omitting target → zero over-coverage
#[test] fn input_too_large_error()              // exceeding max_input_entries returns error
#[test] fn target_not_binding_stats()           // target > lossless count → target_binding=false
#[test] fn progress_callback_cancellation()     // returning Break stops optimization
```

### Differential Tests
```rust
/// Lossless output must match ipnet::aggregate() for all inputs
#[test] fn differential_vs_ipnet_aggregate() {
    // For random inputs, verify our lossless == ipnet::aggregate()
}
```

### Property Tests
```rust
proptest! {
    fn all_inputs_covered(prefixes, target) { /* every input IP in some output */ }
    fn target_respected(prefixes, target) { /* output count ≤ target */ }
    fn over_coverage_non_negative(prefixes, target) { /* no negative over-coverage */ }
    fn lossless_zero_overcoverage(prefixes) { /* lossless mode → 0 over-coverage */ }
    fn greedy_leq_bruteforce(prefixes in vec(arb_ipv4net(), 1..20), target in 1..10usize) {
        /* for small inputs: greedy total_over_coverage ≤ brute_force total_over_coverage */
    }
    fn lossless_matches_ipnet(prefixes in vec(arb_ipv4net(), 1..1000)) {
        /* our lossless == ipnet::aggregate() */
    }
}
```

### Fuzz Targets
```rust
// fuzz/fuzz_targets/fuzz_parse.rs
fuzz_target!(|data: &[u8]| {
    let _ = parse_input(data, false, 100_000);
});

// fuzz/fuzz_targets/fuzz_optimize.rs
#[derive(Arbitrary)]
struct FuzzInput { prefixes: Vec<Ipv4Net>, target: u8 }
fuzz_target!(|input: FuzzInput| {
    let config = OptimizerConfig { ipv4_target: Some(input.target.max(1) as usize), ..Default::default() };
    let nets: Vec<IpNet> = input.prefixes.into_iter().map(IpNet::V4).collect();
    let _ = optimize(&nets, &config);
});
```

### Adversarial Inputs
```rust
#[test] fn adversarial_identical_costs()       // all internal nodes have cost=1
#[test] fn adversarial_maximum_cascade()       // collapse root child → invalidates entire tree
#[test] fn adversarial_worst_case_trie()       // alternating-bit addresses maximize depth
#[test] fn adversarial_all_identical()         // 1M copies of same /32
```

## 8. Future Extensions (Out of Scope for v1)

- **Exact O(n×k) tree DP mode** (`--exact`): For inputs where n×k < 100M (e.g., n=50K lossless entries, k=1000 → 50M ops), the cost-additive structure admits a provably optimal solution via tree DP with knapsack merge. Would provide guaranteed-optimal results at the cost of O(n×k) time and space vs the greedy's O(k log k). Candidate for a `--exact` flag.
- **Radix sort threshold for small inputs**: For n < ~10K entries, comparison sort (`sort_unstable_by`) is faster than 17-pass radix-256 for IPv6 (fewer cache misses, lower constant factor). Add an adaptive threshold that selects comparison sort for small inputs and radix sort for large inputs.
- **Exclusion lists**: "never include these ranges" (e.g., RFC 1918)
- **Incremental updates**: add/remove without full recomputation (trie serialization)
- **Parallel processing**: rayon for IPv4/IPv6 in parallel
- **Minimax objective**: minimize max per-prefix over-coverage
- **Weighted over-coverage**: some ranges costlier to over-cover
- **Pareto frontier**: `--pareto` flag showing (target, over-coverage) curve
- **Terraform output format**: HCL blocks for `aws_ec2_managed_prefix_list_entry`
- **Async reader API**: `optimize_from_async_reader(impl AsyncBufRead)` for service integration
- **Shell completions**: `clap_complete` for bash/zsh/fish
