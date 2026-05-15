use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::trie::{BinaryTrie, NodeIdx, INVALID};

/// Efficiency key comparing cost/savings via widening 160-bit multiplication.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EfficiencyKey {
    pub cost: u128,
    pub savings: u32,
}

impl Ord for EfficiencyKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let lhs = widening_mul_u128_u32(self.cost, other.savings);
        let rhs = widening_mul_u128_u32(other.cost, self.savings);
        lhs.cmp(&rhs)
    }
}

impl PartialOrd for EfficiencyKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Multiply u128 by u32, returning (high_u64, low_u128) for exact 160-bit comparison.
#[inline]
pub fn widening_mul_u128_u32(a: u128, b: u32) -> (u64, u128) {
    let a_lo = a as u64 as u128;
    let a_hi = a >> 64;
    let b = b as u128;

    let prod_lo = a_lo * b;
    let prod_hi = a_hi * b;

    let (low, carry) = prod_lo.overflowing_add(prod_hi << 64);
    let high = (prod_hi >> 64) as u64 + carry as u64;
    (high, low)
}

type HeapEntry = Reverse<(EfficiencyKey, u32, u32)>;

fn efficiency_key(cost: u128, leaf_count: u32) -> EfficiencyKey {
    debug_assert!(leaf_count >= 2);
    EfficiencyKey { cost, savings: leaf_count - 1 }
}

/// Run greedy collapse on the trie until target is met or ratio exceeded.
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
    let mut current_over_coverage: u128 = 0;

    // Initialize heap with all internal nodes having leaf_count >= 2
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

        // Bounds check: skip corrupted/invalid entries
        if node_idx as usize >= trie.arena.len() {
            continue;
        }

        let node = &trie.arena[node_idx as usize];
        if node.is_leaf || node.generation != gen {
            // Heap compaction when stale entries dominate
            if heap.len() > 4 * remaining {
                let arena = &trie.arena;
                heap = BinaryHeap::from(
                    heap.into_vec()
                        .into_iter()
                        .filter(|Reverse((_, idx, g))| {
                            let i = *idx as usize;
                            i < arena.len() && {
                                let n = &arena[i];
                                !n.is_leaf && n.generation == *g
                            }
                        })
                        .collect::<Vec<_>>(),
                );
            }
            continue;
        }

        let leaf_count = node.leaf_count;
        debug_assert!(leaf_count >= 2);
        let reduction = leaf_count as usize - 1;

        let cost = trie.collapse_cost(node_idx);
        let descendant_collapsed_cost = trie.arena[node_idx as usize].collapsed_cost_sum;
        debug_assert!(cost >= descendant_collapsed_cost);
        let net_new_over = cost.saturating_sub(descendant_collapsed_cost);

        // Check ratio before collapsing
        if let Some(max_ratio) = max_ratio {
            if input_covered_ips > 0 {
                let new_total = current_over_coverage.saturating_add(net_new_over);
                if exceeds_ratio(new_total, input_covered_ips, max_ratio) {
                    break;
                }
            }
        }

        current_over_coverage = current_over_coverage.saturating_add(net_new_over);

        trie.invalidate_subtree(node_idx);
        trie.collapse(node_idx);
        remaining = remaining.saturating_sub(reduction);

        // Update ancestors
        let mut ancestor = trie.arena[node_idx as usize].parent;
        while ancestor != INVALID {
            trie.update_leaf_count(ancestor);
            trie.arena[ancestor as usize].collapsed_cost_sum = trie.arena[ancestor as usize]
                .collapsed_cost_sum
                .saturating_add(net_new_over);
            trie.arena[ancestor as usize].generation =
                trie.arena[ancestor as usize].generation.wrapping_add(1);
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

/// Integer-scaled ratio check for overflow safety with large IPv6 values.
pub fn exceeds_ratio(over: u128, covered: u128, max_ratio: f64) -> bool {
    if over <= u64::MAX as u128 && covered <= u64::MAX as u128 {
        return (over as f64) > max_ratio * (covered as f64);
    }
    const SCALE: u128 = 1_000_000;
    let threshold = (max_ratio * SCALE as f64) as u128;
    match over.checked_mul(SCALE) {
        Some(scaled_over) => match threshold.checked_mul(covered) {
            Some(rhs) => scaled_over > rhs,
            None => false,
        },
        None => (over as f64 / covered as f64) > max_ratio,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lossless::ProvenancePrefix;

    #[test]
    fn efficiency_key_ordering() {
        // cost=6/savings=3 (eff=2) < cost=3/savings=1 (eff=3)
        let a = EfficiencyKey { cost: 6, savings: 3 };
        let b = EfficiencyKey { cost: 3, savings: 1 };
        assert!(a < b);
    }

    #[test]
    fn efficiency_key_equal() {
        let a = EfficiencyKey { cost: 6, savings: 2 };
        let b = EfficiencyKey { cost: 9, savings: 3 };
        assert_eq!(a.cmp(&b), std::cmp::Ordering::Equal);
    }

    #[test]
    fn exceeds_ratio_basic() {
        assert!(exceeds_ratio(6, 100, 0.05));
        assert!(!exceeds_ratio(4, 100, 0.05));
        assert!(!exceeds_ratio(5, 100, 0.05));
    }

    #[test]
    fn exceeds_ratio_zero_covered() {
        // With covered=0, ratio check should not be triggered (handled by caller)
        // But if called, should not panic
        assert!(!exceeds_ratio(0, 0, 0.05));
    }

    #[test]
    fn optimize_small_trie_to_target() {
        let entries = vec![
            ProvenancePrefix { prefix: "10.0.0.0/25".parse().unwrap(), source_indices: vec![0], coverage: 128 },
            ProvenancePrefix { prefix: "10.0.0.128/25".parse().unwrap(), source_indices: vec![1], coverage: 128 },
            ProvenancePrefix { prefix: "10.0.1.0/25".parse().unwrap(), source_indices: vec![2], coverage: 128 },
            ProvenancePrefix { prefix: "10.0.1.128/25".parse().unwrap(), source_indices: vec![3], coverage: 128 },
        ];
        let mut trie = BinaryTrie::build_from_v4(&entries).unwrap();
        assert_eq!(trie.total_leaf_count(), 4);

        let _leaves = optimize_trie(&mut trie, 2, None, 512);
        assert!(trie.total_leaf_count() <= 2);
    }

    #[test]
    fn optimize_target_already_met() {
        let entries = vec![
            ProvenancePrefix { prefix: "10.0.0.0/24".parse().unwrap(), source_indices: vec![0], coverage: 256 },
            ProvenancePrefix { prefix: "10.0.1.0/24".parse().unwrap(), source_indices: vec![1], coverage: 256 },
        ];
        let mut trie = BinaryTrie::build_from_v4(&entries).unwrap();
        let _leaves = optimize_trie(&mut trie, 5, None, 512);
        assert_eq!(trie.total_leaf_count(), 2);
    }

    #[test]
    fn optimize_with_ratio_cap() {
        // 4 /32 entries scattered — collapsing would create huge over-coverage
        let entries = vec![
            ProvenancePrefix { prefix: "10.0.0.1/32".parse().unwrap(), source_indices: vec![0], coverage: 1 },
            ProvenancePrefix { prefix: "10.0.0.2/32".parse().unwrap(), source_indices: vec![1], coverage: 1 },
            ProvenancePrefix { prefix: "192.168.0.1/32".parse().unwrap(), source_indices: vec![2], coverage: 1 },
            ProvenancePrefix { prefix: "192.168.0.2/32".parse().unwrap(), source_indices: vec![3], coverage: 1 },
        ];
        let mut trie = BinaryTrie::build_from_v4(&entries).unwrap();
        // With very strict ratio (0.0), no merging should happen
        let _leaves = optimize_trie(&mut trie, 1, Some(0.0), 4);
        // Should still have 4 leaves since ratio=0 prevents any merge
        assert_eq!(trie.total_leaf_count(), 4);
    }

    #[test]
    fn widening_mul_basic() {
        assert_eq!(widening_mul_u128_u32(1, 1), (0, 1));
        assert_eq!(widening_mul_u128_u32(u128::MAX, 1), (0, u128::MAX));
        let (hi, lo) = widening_mul_u128_u32(u128::MAX, 2);
        // u128::MAX * 2 = 2^129 - 2 = (1, 2^128 - 2) in (high, low)
        assert_eq!(hi, 1);
        assert_eq!(lo, u128::MAX - 1);
    }
}
