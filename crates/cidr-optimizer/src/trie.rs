use ipnet::{Ipv4Net, Ipv6Net};

use crate::error::OptimizeError;
use crate::lossless::ProvenancePrefix;

pub type NodeIdx = u32;
pub const INVALID: NodeIdx = u32::MAX;

#[repr(C)]
#[derive(Clone)]
pub struct TrieNode {
    pub skip_bits: u128,
    pub coverage: u128,
    pub collapsed_cost_sum: u128,
    pub children: [NodeIdx; 2],
    pub parent: NodeIdx,
    pub leaf_count: u32,
    pub generation: u32,
    pub depth: u8,
    pub is_leaf: bool,
    pub skip_len: u8,
    _pad: [u8; 9],
}

const _: () = assert!(std::mem::size_of::<TrieNode>() == 80);

impl Default for TrieNode {
    fn default() -> Self {
        Self {
            skip_bits: 0,
            coverage: 0,
            collapsed_cost_sum: 0,
            children: [INVALID, INVALID],
            parent: INVALID,
            leaf_count: 0,
            generation: 0,
            depth: 0,
            is_leaf: false,
            skip_len: 0,
            _pad: [0; 9],
        }
    }
}

pub struct BinaryTrie {
    pub arena: Vec<TrieNode>,
    pub root: NodeIdx,
    pub addr_bits: u8,
}

impl BinaryTrie {
    fn alloc_node(&mut self) -> Result<NodeIdx, OptimizeError> {
        let idx: NodeIdx = self.arena.len().try_into().map_err(|_| OptimizeError::ArenaOverflow)?;
        if idx == INVALID {
            return Err(OptimizeError::ArenaOverflow);
        }
        self.arena.push(TrieNode::default());
        Ok(idx)
    }

    pub fn capacity(&self, node_idx: NodeIdx) -> u128 {
        let node = &self.arena[node_idx as usize];
        if node.depth == 0 && node.skip_len == 0 {
            return u128::MAX;
        }
        let prefix_len = node.depth as u16 + node.skip_len as u16;
        debug_assert!(prefix_len <= self.addr_bits as u16);
        if prefix_len == self.addr_bits as u16 {
            return 1;
        }
        let shift = self.addr_bits as u16 - prefix_len;
        if shift >= 128 {
            return u128::MAX;
        }
        1u128 << shift
    }

    pub fn collapse_cost(&self, node_idx: NodeIdx) -> u128 {
        self.capacity(node_idx).saturating_sub(self.arena[node_idx as usize].coverage)
    }

    pub fn collapse(&mut self, node_idx: NodeIdx) {
        let node = &mut self.arena[node_idx as usize];
        node.is_leaf = true;
        node.leaf_count = 1;
        node.children = [INVALID, INVALID];
    }

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

    pub fn update_leaf_count(&mut self, node_idx: NodeIdx) {
        let node = &self.arena[node_idx as usize];
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

    pub fn total_leaf_count(&self) -> usize {
        self.arena[self.root as usize].leaf_count as usize
    }

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
        // Walk from root to node, collecting bits
        let mut path = Vec::new();
        let mut cur = node_idx;
        while cur != self.root {
            path.push(cur);
            cur = self.arena[cur as usize].parent;
        }
        path.reverse();

        let mut bits: u128 = 0;
        let mut pos: u16 = 0;

        for &idx in &path {
            let node = &self.arena[idx as usize];
            let parent = node.parent;
            // Determine which branch bit was taken at parent
            if parent != INVALID {
                let p = &self.arena[parent as usize];
                let branch_bit = if p.children[1] == idx { 1u128 } else { 0u128 };
                if pos < 128 {
                    bits |= branch_bit << (127 - pos);
                }
                pos += 1;
            }
            // Add skip_bits
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

    pub fn extract_leaves_v4(&self) -> Vec<ProvenancePrefix<Ipv4Net>> {
        let leaf_indices = self.extract_leaf_indices();
        leaf_indices
            .into_iter()
            .map(|idx| {
                let (bits, prefix_len) = self.node_prefix_bits(idx);
                let addr_bits = (bits >> 96) as u32;
                let net = Ipv4Net::new(std::net::Ipv4Addr::from(addr_bits), prefix_len).unwrap();
                ProvenancePrefix {
                    prefix: net,
                    source_indices: Vec::new(),
                    coverage: self.arena[idx as usize].coverage,
                }
            })
            .collect()
    }

    pub fn extract_leaves_v6(&self) -> Vec<ProvenancePrefix<Ipv6Net>> {
        let leaf_indices = self.extract_leaf_indices();
        leaf_indices
            .into_iter()
            .map(|idx| {
                let (bits, prefix_len) = self.node_prefix_bits(idx);
                let net = Ipv6Net::new(std::net::Ipv6Addr::from(bits), prefix_len).unwrap();
                ProvenancePrefix {
                    prefix: net,
                    source_indices: Vec::new(),
                    coverage: self.arena[idx as usize].coverage,
                }
            })
            .collect()
    }

    pub fn build_from_v4(lossless: &[ProvenancePrefix<Ipv4Net>]) -> Result<Self, OptimizeError> {
        let mut trie = Self {
            arena: Vec::new(),
            root: 0,
            addr_bits: 32,
        };
        let root = trie.alloc_node()?;
        trie.root = root;

        for entry in lossless {
            let addr = u32::from(entry.prefix.network()) as u128;
            let key = addr << 96; // shift to top 32 bits of u128
            let prefix_len = entry.prefix.prefix_len();
            let coverage = if prefix_len == 32 { 1u128 } else { 1u128 << (32 - prefix_len) };
            trie.insert(key, prefix_len, coverage)?;
        }

        trie.compute_metadata(trie.root);
        Ok(trie)
    }

    pub fn build_from_v6(lossless: &[ProvenancePrefix<Ipv6Net>]) -> Result<Self, OptimizeError> {
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
    fn insert(&mut self, key: u128, prefix_len: u8, coverage: u128) -> Result<(), OptimizeError> {
        let mut cur = self.root;
        let mut bit_pos: u16 = 0;

        loop {
            let node = &self.arena[cur as usize];
            let node_skip_len = node.skip_len as u16;

            // Check if we need to traverse/split this node's compressed path
            if node_skip_len > 0 {
                let mut mismatch_offset: u16 = 0;
                let mut matched_all = true;

                while mismatch_offset < node_skip_len {
                    let target_bit_pos = bit_pos + mismatch_offset;
                    if target_bit_pos >= prefix_len as u16 {
                        // Key is shorter than this node's path — need to split
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
                    // Split at mismatch_offset
                    self.split_node(cur, mismatch_offset as u8)?;
                    bit_pos += mismatch_offset;
                } else {
                    bit_pos += node_skip_len;
                }
            }

            // Check if we've reached the target prefix length
            if bit_pos == prefix_len as u16 {
                // This node IS the target — mark as leaf with coverage
                self.arena[cur as usize].is_leaf = true;
                self.arena[cur as usize].coverage = self.arena[cur as usize].coverage.saturating_add(coverage);
                return Ok(());
            }

            // Branch on next bit
            let branch_bit = ((key >> (127 - bit_pos)) & 1) as usize;
            bit_pos += 1;

            let child = self.arena[cur as usize].children[branch_bit];
            if child == INVALID {
                // Create new leaf with remaining bits as skip
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
    fn split_node(&mut self, node_idx: NodeIdx, offset: u8) -> Result<(), OptimizeError> {
        let node = &self.arena[node_idx as usize];
        let old_skip_len = node.skip_len;
        let old_skip_bits = node.skip_bits;
        let old_depth = node.depth;

        debug_assert!(offset < old_skip_len);

        // Create a new child that holds the remainder of the compressed path
        let remainder_node = self.alloc_node()?;

        let remainder_skip_len = old_skip_len - offset - 1; // -1 for the branch bit
        let branch_bit_in_skip = ((old_skip_bits >> (old_skip_len as u16 - 1 - offset as u16)) & 1) as usize;

        let remainder_bits = if remainder_skip_len > 0 {
            old_skip_bits & ((1u128 << remainder_skip_len) - 1)
        } else {
            0
        };

        // Copy the original node's state to the remainder
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

        // Update children's parent pointers
        for &child in &orig_children {
            if child != INVALID {
                self.arena[child as usize].parent = remainder_node;
            }
        }

        // Truncate the original node to just the prefix before the mismatch
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_size_is_80_bytes() {
        assert_eq!(std::mem::size_of::<TrieNode>(), 80);
    }

    #[test]
    fn build_simple_trie_v4() {
        let entries = vec![
            ProvenancePrefix { prefix: "10.0.0.0/24".parse().unwrap(), source_indices: vec![0], coverage: 256 },
            ProvenancePrefix { prefix: "10.0.1.0/24".parse().unwrap(), source_indices: vec![1], coverage: 256 },
        ];
        let trie = BinaryTrie::build_from_v4(&entries).unwrap();
        assert_eq!(trie.total_leaf_count(), 2);
    }

    #[test]
    fn capacity_root_v4() {
        let entries = vec![
            ProvenancePrefix { prefix: "10.0.0.0/24".parse().unwrap(), source_indices: vec![0], coverage: 256 },
        ];
        let trie = BinaryTrie::build_from_v4(&entries).unwrap();
        assert_eq!(trie.capacity(trie.root), u128::MAX);
    }

    #[test]
    fn capacity_leaf_v4() {
        let entries = vec![
            ProvenancePrefix { prefix: "10.0.0.0/24".parse().unwrap(), source_indices: vec![0], coverage: 256 },
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
            ProvenancePrefix { prefix: "10.0.0.1/32".parse().unwrap(), source_indices: vec![0], coverage: 1 },
            ProvenancePrefix { prefix: "10.0.0.2/32".parse().unwrap(), source_indices: vec![1], coverage: 1 },
        ];
        let trie = BinaryTrie::build_from_v4(&entries).unwrap();
        assert_eq!(trie.total_leaf_count(), 2);
        // With path compression, should have far fewer than 32 nodes per entry
        assert!(trie.arena.len() < 10);
    }

    #[test]
    fn collapse_and_extract() {
        let entries = vec![
            ProvenancePrefix { prefix: "10.0.0.0/25".parse().unwrap(), source_indices: vec![0], coverage: 128 },
            ProvenancePrefix { prefix: "10.0.0.128/25".parse().unwrap(), source_indices: vec![1], coverage: 128 },
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
            ProvenancePrefix { prefix: "2001:db8::/48".parse().unwrap(), source_indices: vec![0], coverage: 1u128 << 80 },
            ProvenancePrefix { prefix: "2001:db8:1::/48".parse().unwrap(), source_indices: vec![1], coverage: 1u128 << 80 },
        ];
        let trie = BinaryTrie::build_from_v6(&entries).unwrap();
        assert_eq!(trie.total_leaf_count(), 2);
    }
}
