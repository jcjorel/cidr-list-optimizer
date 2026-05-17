//! Path-compressed binary trie for CIDR budget optimization.
//!
//! Provides an arena-allocated binary trie that stores IPv4/IPv6 prefixes with
//! path compression. Used by the budget optimizer to find the least-cost
//! collapse operations that reduce entry count while minimizing over-coverage.

use ipnet::{Ipv4Net, Ipv6Net};

use crate::error::OptimizeError;
use crate::exclusion::ExclusionSet;
use crate::lossless::SourceMapPrefix;
use crate::preferred::PreferredSet;

/// Index into the trie arena; u32 for compact storage.
pub type NodeIdx = u32;
/// Sentinel value indicating no node (null pointer equivalent).
pub const INVALID: NodeIdx = u32::MAX;

/// A single node in the path-compressed binary trie.
///
/// Each node represents a prefix position and may store a compressed path
/// segment (`skip_bits`/`skip_len`) to avoid allocating intermediate nodes
/// for long shared prefixes.
#[repr(C)]
#[derive(Clone)]
pub struct TrieNode {
    /// Compressed path bits for this node's skip segment (stored in the lowest skip_len bits,
    /// MSB of the path in the highest position among those bits).
    pub skip_bits: u128,
    /// Total IP address coverage of original input leaves under this node (not updated on collapse).
    pub coverage: u128,
    /// Accumulated collapse cost of all descendants already collapsed.
    pub collapsed_cost_sum: u128,
    /// Number of preferred-set IPs overlapping this subtree's coverage.
    pub preferred_overlap: u128,
    /// Preferred overlap restricted to actually-covered addresses.
    pub preferred_overlap_in_coverage: u128,
    /// Child node indices: [0] for bit=0, [1] for bit=1.
    pub children: [NodeIdx; 2],
    /// Parent node index (INVALID for root).
    pub parent: NodeIdx,
    /// Number of leaf nodes in this subtree.
    pub leaf_count: u32,
    /// Generation counter for invalidation tracking.
    pub generation: u32,
    /// Bit depth of this node within the trie (excludes skip_len).
    pub depth: u8,
    /// Whether this node represents an actual prefix (leaf) vs internal branch.
    pub is_leaf: bool,
    /// Length of the compressed path segment stored in skip_bits.
    pub skip_len: u8,
    /// Whether collapsing this node would expose excluded IP ranges.
    pub is_excluded: bool,
    // Explicit padding to reach 112-byte cache-aligned size
    _pad: [u8; 4],
}

// Compile-time size assertion for cache-line alignment
const _: () = assert!(std::mem::size_of::<TrieNode>() == 112);

impl Default for TrieNode {
    fn default() -> Self {
        Self {
            skip_bits: 0,
            coverage: 0,
            collapsed_cost_sum: 0,
            preferred_overlap: 0,
            preferred_overlap_in_coverage: 0,
            children: [INVALID; 2],
            parent: INVALID,
            leaf_count: 0,
            generation: 0,
            depth: 0,
            is_leaf: false,
            skip_len: 0,
            is_excluded: false,
            _pad: [0; 4],
        }
    }
}

/// Arena-allocated path-compressed binary trie for IPv4/IPv6 prefixes.
///
/// Stores all nodes in a flat `Vec` for cache-friendly traversal. Supports
/// insertion, collapse, exclusion marking, and leaf extraction for both
/// address families using a uniform u128 key representation.
pub struct BinaryTrie {
    /// Flat arena holding all trie nodes; indexed by `NodeIdx`.
    pub arena: Vec<TrieNode>,
    /// Index of the root node in the arena.
    pub root: NodeIdx,
    /// Address family bit width (32 for IPv4, 128 for IPv6).
    pub addr_bits: u8,
}

impl BinaryTrie {
    fn alloc_node(&mut self) -> Result<NodeIdx, OptimizeError> {
        let idx: NodeIdx = self.arena.len().try_into().map_err(|_| OptimizeError::ArenaOverflow)?;
        // Returning INVALID as a live index would corrupt parent/child pointers
        if idx == INVALID {
            return Err(OptimizeError::ArenaOverflow);
        }
        self.arena.push(TrieNode::default());
        Ok(idx)
    }

    /// Returns the total number of IP addresses in this node's prefix range.
    ///
    /// For the root node (depth=0, skip_len=0) this returns u128::MAX as a sentinel
    /// because the full address space (2^addr_bits) overflows u128. For a /32 IPv4
    /// or /128 IPv6 prefix it is exactly 1.
    pub fn capacity(&self, node_idx: NodeIdx) -> u128 {
        let node = &self.arena[node_idx as usize];
        // Entire address space — cannot be expressed as 2^N without overflow
        if node.depth == 0 && node.skip_len == 0 {
            return u128::MAX;
        }
        let prefix_len = node.depth as u16 + node.skip_len as u16;
        debug_assert!(prefix_len <= self.addr_bits as u16);
        if prefix_len == self.addr_bits as u16 {
            return 1;
        }
        let shift = self.addr_bits as u16 - prefix_len;
        // Defensive: shift >= 128 would overflow u128; shouldn't occur in valid tries
        if shift >= 128 {
            return u128::MAX;
        }
        1u128 << shift
    }

    /// Returns the over-coverage cost of collapsing this node into a single leaf.
    ///
    /// This is the number of extra IP addresses that would be allowed beyond
    /// what the original leaves already cover.
    pub fn collapse_cost(&self, node_idx: NodeIdx) -> u128 {
        self.capacity(node_idx).saturating_sub(self.arena[node_idx as usize].coverage)
    }

    /// Collapses this node into a leaf, discarding all children.
    ///
    /// After collapse, the node represents its entire prefix range as a single
    /// entry. Callers must update ancestor leaf counts separately.
    pub fn collapse(&mut self, node_idx: NodeIdx) {
        let node = &mut self.arena[node_idx as usize];
        node.is_leaf = true;
        node.leaf_count = 1;
        node.children = [INVALID, INVALID];
    }

    /// Invalidates all descendants by incrementing their generation counter.
    ///
    /// Used to mark heap entries as stale after a collapse operation without
    /// removing them from the priority queue.
    pub fn invalidate_subtree(&mut self, node_idx: NodeIdx) {
        let mut stack = vec![node_idx];
        while let Some(idx) = stack.pop() {
            let children = self.arena[idx as usize].children;
            for &child in &children {
                if child != INVALID {
                    self.arena[child as usize].generation =
                        self.arena[child as usize].generation.wrapping_add(1);
                    stack.push(child);
                }
            }
        }
    }

    /// Recomputes `leaf_count` for a node from its immediate children.
    ///
    /// Must be called bottom-up after structural changes (collapse, split).
    pub fn update_leaf_count(&mut self, node_idx: NodeIdx) {
        let node = &self.arena[node_idx as usize];
        // Leaf count is set at insertion/collapse time, not recomputed here
        if node.is_leaf {
            return;
        }
        let children = node.children;
        let mut count = 0u32;
        for &child in &children {
            if child != INVALID {
                count += self.arena[child as usize].leaf_count;
            }
        }
        self.arena[node_idx as usize].leaf_count = count;
    }

    /// Returns the total number of leaf prefixes in the trie.
    pub fn total_leaf_count(&self) -> usize {
        self.arena[self.root as usize].leaf_count as usize
    }

    /// Collects all leaf node indices via DFS traversal.
    pub fn extract_leaf_indices(&self) -> Vec<NodeIdx> {
        let mut leaves = Vec::new();
        self.collect_leaves(self.root, &mut leaves);
        leaves
    }

    fn collect_leaves(&self, node_idx: NodeIdx, out: &mut Vec<NodeIdx>) {
        let node = &self.arena[node_idx as usize];
        if node.is_leaf {
            out.push(node_idx);
            return;
        }
        for &child in &node.children {
            if child != INVALID {
                self.collect_leaves(child, out);
            }
        }
    }

    /// Extract the prefix (network bits + prefix_len) for a node.
    fn node_prefix_bits(&self, node_idx: NodeIdx) -> (u128, u8) {
        // Walk from node up to root, then reverse to get root-to-node order
        let mut path = Vec::new();
        let mut cur = node_idx;
        while cur != self.root {
            path.push(cur);
            cur = self.arena[cur as usize].parent;
        }
        // Replay path top-down to reconstruct prefix bits in MSB-first order
        path.reverse();

        let mut bits: u128 = 0;
        let mut pos: u16 = 0;

        for &idx in &path {
            let node = &self.arena[idx as usize];
            let parent = node.parent;
            if parent != INVALID {
                let p = &self.arena[parent as usize];
                let branch_bit = if p.children[1] == idx { 1u128 } else { 0u128 };
                if pos < 128 {
                    bits |= branch_bit << (127 - pos);
                }
                pos += 1;
            }
            let skip_len = node.skip_len as u16;
            for i in 0..skip_len {
                let bit = (node.skip_bits >> (skip_len - 1 - i)) & 1;
                if pos < 128 {
                    bits |= bit << (127 - pos);
                }
                pos += 1;
            }
        }

        let prefix_len = pos.min(self.addr_bits as u16) as u8;
        (bits, prefix_len)
    }

    /// Extracts all leaf prefixes as IPv4 networks with their coverage metadata.
    pub fn extract_leaves_v4(&self) -> Vec<SourceMapPrefix<Ipv4Net>> {
        let leaf_indices = self.extract_leaf_indices();
        leaf_indices
            .into_iter()
            .map(|idx| {
                let (bits, prefix_len) = self.node_prefix_bits(idx);
                let addr_bits = (bits >> 96) as u32;
                let net = Ipv4Net::new(std::net::Ipv4Addr::from(addr_bits), prefix_len).unwrap();
                SourceMapPrefix {
                    prefix: net,
                    source_indices: Vec::new(),
                    coverage: self.arena[idx as usize].coverage,
                    preferred_overlap_in_coverage: self.arena[idx as usize].preferred_overlap_in_coverage,
                }
            })
            .collect()
    }

    /// Extracts all leaf prefixes as IPv6 networks with their coverage metadata.
    pub fn extract_leaves_v6(&self) -> Vec<SourceMapPrefix<Ipv6Net>> {
        let leaf_indices = self.extract_leaf_indices();
        leaf_indices
            .into_iter()
            .map(|idx| {
                let (bits, prefix_len) = self.node_prefix_bits(idx);
                let net = Ipv6Net::new(std::net::Ipv6Addr::from(bits), prefix_len).unwrap();
                SourceMapPrefix {
                    prefix: net,
                    source_indices: Vec::new(),
                    coverage: self.arena[idx as usize].coverage,
                    preferred_overlap_in_coverage: self.arena[idx as usize].preferred_overlap_in_coverage,
                }
            })
            .collect()
    }

    /// Builds a trie from pre-aggregated IPv4 prefixes.
    ///
    /// Inserts each prefix and computes bottom-up metadata (coverage, leaf counts).
    pub fn build_from_v4(lossless: &[SourceMapPrefix<Ipv4Net>]) -> Result<Self, OptimizeError> {
        let mut trie = Self {
            arena: Vec::new(),
            root: 0,
            addr_bits: 32,
        };
        let root = trie.alloc_node()?;
        trie.root = root;

        // Insert each lossless prefix into the trie
        for entry in lossless {
            let addr = u32::from(entry.prefix.network()) as u128;
            // Pack 32-bit IPv4 address into top bits of u128 for uniform trie key format
            let key = addr << 96;
            let prefix_len = entry.prefix.prefix_len();
            let coverage = if prefix_len == 32 { 1u128 } else { 1u128 << (32 - prefix_len) };
            trie.insert(key, prefix_len, coverage)?;
        }

        trie.compute_metadata(trie.root);
        Ok(trie)
    }

    /// Builds a trie from pre-aggregated IPv6 prefixes.
    ///
    /// Inserts each prefix and computes bottom-up metadata (coverage, leaf counts).
    pub fn build_from_v6(lossless: &[SourceMapPrefix<Ipv6Net>]) -> Result<Self, OptimizeError> {
        let mut trie = Self {
            arena: Vec::new(),
            root: 0,
            addr_bits: 128,
        };
        let root = trie.alloc_node()?;
        trie.root = root;

        for entry in lossless {
            let key = u128::from(entry.prefix.network());
            let prefix_len = entry.prefix.prefix_len();
            let coverage = if prefix_len == 128 { 1u128 } else { 1u128 << (128 - prefix_len) };
            trie.insert(key, prefix_len, coverage)?;
        }

        trie.compute_metadata(trie.root);
        Ok(trie)
    }

    /// Insert a prefix into the trie. Key is stored in the top `addr_bits` bits of u128.
    ///
    /// Algorithm: walks the trie from root following the key bits. At each node,
    /// compares the key against the node's compressed path (skip_bits). On mismatch,
    /// splits the node at the divergence point. On full match, descends to the
    /// appropriate child or creates a new leaf if the slot is empty.
    fn insert(&mut self, key: u128, prefix_len: u8, coverage: u128) -> Result<(), OptimizeError> {
        let mut cur = self.root;
        let mut bit_pos: u16 = 0;

        loop {
            let node = &self.arena[cur as usize];
            let node_skip_len = node.skip_len as u16;

            // Compare key bits against this node's compressed path segment
            if node_skip_len > 0 {
                let mut mismatch_offset: u16 = 0;
                let mut matched_all = true;

                while mismatch_offset < node_skip_len {
                    let target_bit_pos = bit_pos + mismatch_offset;
                    // Key is shorter than this node's path — need to split
                    if target_bit_pos >= prefix_len as u16 {
                        matched_all = false;
                        break;
                    }
                    let key_bit = (key >> (127 - target_bit_pos)) & 1;
                    let skip_bit = (node.skip_bits >> (node_skip_len - 1 - mismatch_offset)) & 1;
                    if key_bit != skip_bit {
                        matched_all = false;
                        break;
                    }
                    mismatch_offset += 1;
                }

                if !matched_all {
                    self.split_node(cur, mismatch_offset as u8)?;
                    bit_pos += mismatch_offset;
                } else {
                    bit_pos += node_skip_len;
                }
            }

            if bit_pos == prefix_len as u16 {
                self.arena[cur as usize].is_leaf = true;
                self.arena[cur as usize].coverage = self.arena[cur as usize].coverage.saturating_add(coverage);
                return Ok(());
            }

            let branch_bit = ((key >> (127 - bit_pos)) & 1) as usize;
            bit_pos += 1;

            let child = self.arena[cur as usize].children[branch_bit];
            // Store remaining key bits as the new leaf's compressed path
            if child == INVALID {
                let new_node = self.alloc_node()?;
                let remaining = prefix_len as u16 - bit_pos;
                let skip_bits = if remaining > 0 && remaining < 128 {
                    (key >> (128 - bit_pos - remaining)) & ((1u128 << remaining) - 1)
                } else {
                    0
                };
                self.arena[new_node as usize].depth = bit_pos as u8;
                self.arena[new_node as usize].skip_len = remaining as u8;
                self.arena[new_node as usize].skip_bits = skip_bits;
                self.arena[new_node as usize].parent = cur;
                self.arena[new_node as usize].is_leaf = true;
                self.arena[new_node as usize].coverage = coverage;
                self.arena[new_node as usize].leaf_count = 1;
                self.arena[cur as usize].children[branch_bit] = new_node;
                return Ok(());
            }

            cur = child;
        }
    }

    /// Split a compressed node at the given offset within its skip segment.
    ///
    /// Algorithm: creates a "remainder" node that inherits the original's children
    /// and state from the split point onward. The original node is truncated to
    /// hold only the prefix before the split, with the remainder as its child
    /// on the appropriate branch.
    fn split_node(&mut self, node_idx: NodeIdx, offset: u8) -> Result<(), OptimizeError> {
        let node = &self.arena[node_idx as usize];
        let old_skip_len = node.skip_len;
        let old_skip_bits = node.skip_bits;
        let old_depth = node.depth;

        debug_assert!(offset < old_skip_len);

        // Allocate remainder node to hold the suffix of the compressed path
        let remainder_node = self.alloc_node()?;

        // -1 because the bit at the split point becomes the branch direction, not part of either skip segment
        let remainder_skip_len = old_skip_len - offset - 1;
        // The bit at the split offset determines which child slot gets the remainder
        let branch_bit_in_skip = ((old_skip_bits >> (old_skip_len as u16 - 1 - offset as u16)) & 1) as usize;

        let remainder_bits = if remainder_skip_len > 0 {
            old_skip_bits & ((1u128 << remainder_skip_len) - 1)
        } else {
            0
        };

        // Transfer the original node's state to the remainder
        let orig = &self.arena[node_idx as usize];
        let orig_children = orig.children;
        let orig_is_leaf = orig.is_leaf;
        let orig_coverage = orig.coverage;
        let orig_leaf_count = orig.leaf_count;
        let orig_collapsed_cost_sum = orig.collapsed_cost_sum;

        let new_depth = old_depth as u16 + offset as u16 + 1;
        self.arena[remainder_node as usize].depth = new_depth as u8;
        self.arena[remainder_node as usize].skip_len = remainder_skip_len;
        self.arena[remainder_node as usize].skip_bits = remainder_bits;
        self.arena[remainder_node as usize].parent = node_idx;
        self.arena[remainder_node as usize].children = orig_children;
        self.arena[remainder_node as usize].is_leaf = orig_is_leaf;
        self.arena[remainder_node as usize].coverage = orig_coverage;
        self.arena[remainder_node as usize].leaf_count = orig_leaf_count;
        self.arena[remainder_node as usize].collapsed_cost_sum = orig_collapsed_cost_sum;

        for &child in &orig_children {
            if child != INVALID {
                self.arena[child as usize].parent = remainder_node;
            }
        }

        let new_skip_len = offset;
        let new_skip_bits = if new_skip_len > 0 {
            old_skip_bits >> (old_skip_len as u16 - offset as u16)
        } else {
            0
        };

        self.arena[node_idx as usize].skip_len = new_skip_len;
        self.arena[node_idx as usize].skip_bits = new_skip_bits;
        self.arena[node_idx as usize].children = [INVALID, INVALID];
        self.arena[node_idx as usize].children[branch_bit_in_skip] = remainder_node;
        self.arena[node_idx as usize].is_leaf = false;
        self.arena[node_idx as usize].coverage = orig_coverage; // will be recomputed
        self.arena[node_idx as usize].leaf_count = orig_leaf_count;

        Ok(())
    }

    /// Bottom-up computation of coverage and leaf_count.
    fn compute_metadata(&mut self, node_idx: NodeIdx) -> (u128, u32) {
        let is_leaf = self.arena[node_idx as usize].is_leaf;
        if is_leaf {
            self.arena[node_idx as usize].leaf_count = 1;
            let coverage = self.arena[node_idx as usize].coverage;
            return (coverage, 1);
        }

        let children = self.arena[node_idx as usize].children;
        let mut total_coverage = 0u128;
        let mut total_leaves = 0u32;

        for &child in &children {
            if child != INVALID {
                let (cov, leaves) = self.compute_metadata(child);
                total_coverage = total_coverage.saturating_add(cov);
                total_leaves += leaves;
            }
        }

        self.arena[node_idx as usize].coverage = total_coverage;
        self.arena[node_idx as usize].leaf_count = total_leaves;
        (total_coverage, total_leaves)
    }

    /// Mark internal nodes whose collapse would cover excluded IPs not already
    /// covered by input leaves. Ancestors of excluded nodes are also excluded.
    ///
    /// Two-pass approach: first scans all internal nodes and marks those whose
    /// prefix range intersects an exclusion zone (and has uncovered gaps). Then
    /// propagates the exclusion flag upward to all ancestors so the optimizer
    /// never collapses a node that would absorb an excluded range.
    pub fn mark_exclusions(&mut self, exclusion_set: &ExclusionSet, is_v4: bool) {
        // Select address-family-specific intersection check to avoid branching in the loop
        let check_fn: fn(&ExclusionSet, u128, u128) -> bool = if is_v4 {
            ExclusionSet::intersects_v4
        } else {
            ExclusionSet::intersects_v6
        };

        // Pass 1: mark internal nodes with leaf_count >= 2 that intersect exclusions
        for idx in 0..self.arena.len() {
            let node = &self.arena[idx];
            // Skip leaves and nodes that can't be collapsed (single-leaf subtrees)
            if node.is_leaf || node.leaf_count < 2 {
                continue;
            }

            let (bits, prefix_len) = self.node_prefix_bits(idx as u32);
            let (start, end) = self.interval_from_prefix(bits, prefix_len);

            if !check_fn(exclusion_set, start, end) {
                continue;
            }

            // If all IPs in this range are already input-covered, collapsing adds no new exposure
            let capacity = self.capacity(idx as u32);
            let coverage = self.arena[idx].coverage;
            if coverage >= capacity {
                continue;
            }

            self.arena[idx].is_excluded = true;
        }

        // Pass 2: propagate exclusion upward to all ancestors
        // Separate pass ensures all direct exclusion marks from Pass 1 are complete before propagating to ancestors
        for idx in 0..self.arena.len() {
            if !self.arena[idx].is_excluded {
                continue;
            }
            let mut ancestor = self.arena[idx].parent;
            while ancestor != INVALID {
                if self.arena[ancestor as usize].is_excluded {
                    break;
                }
                self.arena[ancestor as usize].is_excluded = true;
                ancestor = self.arena[ancestor as usize].parent;
            }
        }
    }

    /// Compute the interval (start, end inclusive) for a prefix given its bits and length.
    fn interval_from_prefix(&self, bits: u128, prefix_len: u8) -> (u128, u128) {
        if self.addr_bits == 32 {
            let addr = (bits >> 96) as u32;
            let start = addr as u128;
            let end = if prefix_len == 32 {
                start
            } else {
                start | ((1u128 << (32 - prefix_len)) - 1)
            };
            (start, end)
        } else {
            let start = bits;
            let end = if prefix_len == 128 {
                start
            } else if prefix_len == 0 {
                u128::MAX
            } else {
                // Fill host bits with 1s
                start | ((1u128 << (128 - prefix_len)) - 1)
            };
            (start, end)
        }
    }

    /// Public: compute the interval (start, end inclusive) for a given node.
    pub fn node_interval(&self, node_idx: NodeIdx) -> (u128, u128) {
        let (bits, prefix_len) = self.node_prefix_bits(node_idx);
        self.interval_from_prefix(bits, prefix_len)
    }

    /// Bottom-up pass: set `preferred_overlap` for each node.
    /// For leaves: overlap of preferred set with the leaf's covered IPs (capped at coverage).
    /// For internal nodes: sum of children's preferred_overlap.
    pub fn mark_preferred_overlaps(&mut self, preferred_set: &PreferredSet, is_v4: bool) {
        self.compute_preferred_overlap(self.root, preferred_set, is_v4);
    }

    fn compute_preferred_overlap(&mut self, node_idx: NodeIdx, preferred_set: &PreferredSet, is_v4: bool) -> u128 {
        let node = &self.arena[node_idx as usize];
        if node.is_leaf {
            let (start, end) = self.node_interval(node_idx);
            let overlap = if is_v4 {
                preferred_set.overlap_count_v4(start, end)
            } else {
                preferred_set.overlap_count_v6(start, end)
            };
            let capped = overlap.min(self.arena[node_idx as usize].coverage);
            self.arena[node_idx as usize].preferred_overlap = capped;
            self.arena[node_idx as usize].preferred_overlap_in_coverage = capped;
            return capped;
        }

        let children = node.children;
        let mut total: u128 = 0;
        for &child in &children {
            if child != INVALID {
                total = total.saturating_add(self.compute_preferred_overlap(child, preferred_set, is_v4));
            }
        }
        self.arena[node_idx as usize].preferred_overlap = total;
        self.arena[node_idx as usize].preferred_overlap_in_coverage = total;
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exclusion::ExclusionSet;
    use crate::types::ExclusionEntry;

    #[test]
    fn node_size_is_112_bytes() {
        assert_eq!(std::mem::size_of::<TrieNode>(), 112);
    }

    #[test]
    fn build_simple_trie_v4() {
        let entries = vec![
            SourceMapPrefix { prefix: "10.0.0.0/24".parse().unwrap(), source_indices: vec![0], coverage: 256, preferred_overlap_in_coverage: 0 },
            SourceMapPrefix { prefix: "10.0.1.0/24".parse().unwrap(), source_indices: vec![1], coverage: 256, preferred_overlap_in_coverage: 0 },
        ];
        let trie = BinaryTrie::build_from_v4(&entries).unwrap();
        assert_eq!(trie.total_leaf_count(), 2);
    }

    #[test]
    fn capacity_root_v4() {
        let entries = vec![
            SourceMapPrefix { prefix: "10.0.0.0/24".parse().unwrap(), source_indices: vec![0], coverage: 256, preferred_overlap_in_coverage: 0 },
        ];
        let trie = BinaryTrie::build_from_v4(&entries).unwrap();
        assert_eq!(trie.capacity(trie.root), u128::MAX);
    }

    #[test]
    fn capacity_leaf_v4() {
        let entries = vec![
            SourceMapPrefix { prefix: "10.0.0.0/24".parse().unwrap(), source_indices: vec![0], coverage: 256, preferred_overlap_in_coverage: 0 },
        ];
        let trie = BinaryTrie::build_from_v4(&entries).unwrap();
        let leaves = trie.extract_leaf_indices();
        assert_eq!(leaves.len(), 1);
        // /24 has capacity 2^8 = 256
        assert_eq!(trie.capacity(leaves[0]), 256);
    }

    #[test]
    fn path_compression_reduces_nodes() {
        // Two /32 entries that share a long prefix should use path compression
        let entries = vec![
            SourceMapPrefix { prefix: "10.0.0.1/32".parse().unwrap(), source_indices: vec![0], coverage: 1, preferred_overlap_in_coverage: 0 },
            SourceMapPrefix { prefix: "10.0.0.2/32".parse().unwrap(), source_indices: vec![1], coverage: 1, preferred_overlap_in_coverage: 0 },
        ];
        let trie = BinaryTrie::build_from_v4(&entries).unwrap();
        assert_eq!(trie.total_leaf_count(), 2);
        // With path compression, should have far fewer than 32 nodes per entry
        assert!(trie.arena.len() < 10);
    }

    #[test]
    fn collapse_and_extract() {
        let entries = vec![
            SourceMapPrefix { prefix: "10.0.0.0/25".parse().unwrap(), source_indices: vec![0], coverage: 128, preferred_overlap_in_coverage: 0 },
            SourceMapPrefix { prefix: "10.0.0.128/25".parse().unwrap(), source_indices: vec![1], coverage: 128, preferred_overlap_in_coverage: 0 },
        ];
        let mut trie = BinaryTrie::build_from_v4(&entries).unwrap();
        assert_eq!(trie.total_leaf_count(), 2);

        // Find the parent of both leaves (should be the node representing /24)
        let leaves = trie.extract_leaf_indices();
        let parent = trie.arena[leaves[0] as usize].parent;
        assert_ne!(parent, INVALID);

        trie.collapse(parent);
        trie.update_leaf_count(parent);
        // Update ancestors
        let mut anc = trie.arena[parent as usize].parent;
        while anc != INVALID {
            trie.update_leaf_count(anc);
            anc = trie.arena[anc as usize].parent;
        }
        assert_eq!(trie.total_leaf_count(), 1);

        let result = trie.extract_leaves_v4();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].prefix, "10.0.0.0/24".parse::<Ipv4Net>().unwrap());
    }

    #[test]
    fn build_v6_trie() {
        let entries = vec![
            SourceMapPrefix { prefix: "2001:db8::/48".parse().unwrap(), source_indices: vec![0], coverage: 1u128 << 80, preferred_overlap_in_coverage: 0 },
            SourceMapPrefix { prefix: "2001:db8:1::/48".parse().unwrap(), source_indices: vec![1], coverage: 1u128 << 80, preferred_overlap_in_coverage: 0 },
        ];
        let trie = BinaryTrie::build_from_v6(&entries).unwrap();
        assert_eq!(trie.total_leaf_count(), 2);
    }

    #[test]
    fn mark_exclusions_blocks_merge() {
        // Two sibling /25s under 10.0.0.0/24 — their parent at /24 can merge them
        let entries = vec![
            SourceMapPrefix { prefix: "10.0.0.0/25".parse().unwrap(), source_indices: vec![0], coverage: 128, preferred_overlap_in_coverage: 0 },
            SourceMapPrefix { prefix: "10.0.0.128/25".parse().unwrap(), source_indices: vec![1], coverage: 128, preferred_overlap_in_coverage: 0 },
        ];

        // Scenario: non-sibling entries where the parent has gaps
        let entries2 = vec![
            SourceMapPrefix { prefix: "10.0.0.0/25".parse().unwrap(), source_indices: vec![0], coverage: 128, preferred_overlap_in_coverage: 0 },
            SourceMapPrefix { prefix: "10.0.1.0/25".parse().unwrap(), source_indices: vec![1], coverage: 128, preferred_overlap_in_coverage: 0 },
        ];
        let mut trie2 = BinaryTrie::build_from_v4(&entries2).unwrap();

        // Exclude 10.0.0.200/32 — in the 10.0.0.128/25 range, not covered by input
        let excl = vec![ExclusionEntry {
            prefix: "10.0.0.200/32".parse().unwrap(),
            source: "test".to_string(),
            comment: None,
        }];
        let set = ExclusionSet::build(&excl);
        trie2.mark_exclusions(&set, true);
        // The parent node that would merge these two /25s covers 10.0.0.0-10.0.1.255
        // which intersects with 10.0.0.200, and coverage < capacity → excluded
        assert!(trie2.arena.iter().any(|n| n.is_excluded));

        // Verify: when both /25s fully cover /24, exclusion within /24 is NOT marked
        // because coverage == capacity
        let mut trie3 = BinaryTrie::build_from_v4(&entries).unwrap();
        let excl_within = vec![ExclusionEntry {
            prefix: "10.0.0.50/32".parse().unwrap(),
            source: "test".to_string(),
            comment: None,
        }];
        let set_within = ExclusionSet::build(&excl_within);
        trie3.mark_exclusions(&set_within, true);
        // The /24 parent has coverage == capacity (256 == 256), so NOT excluded at that level
        // But higher nodes (root etc.) have coverage < capacity and DO intersect → they get excluded
        // The key point: the immediate parent of the two /25s is NOT excluded
        let leaves = trie3.extract_leaf_indices();
        let parent = trie3.arena[leaves[0] as usize].parent;
        assert!(!trie3.arena[parent as usize].is_excluded);
    }
}
