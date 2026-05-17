//! Budget-constrained greedy optimizer that collapses trie nodes to reduce CIDR entry count
//! while bounding over-coverage.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::preferred::PreferredSet;
use crate::trie::{BinaryTrie, NodeIdx, INVALID};

/// Efficiency key for ranking merge candidates by cost-per-entry-saved ratio.
///
/// Ordering uses widening 160-bit cross-multiplication to compare ratios without division.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EfficiencyKey {
    /// Number of extra IP addresses introduced by collapsing this node (lower is better).
    pub cost: u128,
    /// Number of leaf entries eliminated by the collapse (leaf_count - 1).
    pub savings: u32,
}

impl Ord for EfficiencyKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Compare ratios cost/savings cross-multiplied to avoid division
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

    // Combine partial products: low 64 bits of prod_hi overlap with high 64 bits of prod_lo,
    // carry propagates to high word
    let (low, carry) = prod_lo.overflowing_add(prod_hi << 64);
    let high = (prod_hi >> 64) as u64 + carry as u64;
    (high, low)
}

type HeapEntry = Reverse<(EfficiencyKey, u32, u32)>;

/// Build an efficiency key for a collapse candidate.
///
/// Savings is `leaf_count - 1` because collapsing N leaves into 1 node saves N-1 entries.
fn efficiency_key(cost: u128, leaf_count: u32) -> EfficiencyKey {
    debug_assert!(leaf_count >= 2);
    EfficiencyKey { cost, savings: leaf_count - 1 }
}

/// Run greedy collapse on the trie until target is met or ratio exceeded.
///
/// # Arguments
///
/// * `trie` — Mutable binary trie to optimize in-place
/// * `target` — Maximum number of leaf entries allowed after optimization
/// * `max_ratio` — Optional cap on total over-coverage as a fraction of input coverage
/// * `input_covered_ips` — Total IP addresses covered by the original input set
/// * `preferred_set` — IPs considered acceptable over-coverage (discounted from cost)
/// * `max_non_preferred_ratio` — Optional separate cap on non-preferred over-coverage
///
/// # Algorithm
///
/// 1. **Heap initialization**: score every internal node by efficiency (cost per entry saved),
///    discounting preferred overlap when active.
/// 2. **Greedy loop**: pop the cheapest merge candidate, verify it hasn't been invalidated,
///    check ratio caps, then collapse the subtree into a single leaf.
/// 3. **Ancestor propagation**: after each collapse, walk up the parent chain updating leaf
///    counts, collapsed cost sums, preferred metrics, and re-enqueuing ancestors with fresh
///    efficiency scores.
pub fn optimize_trie(
    trie: &mut BinaryTrie,
    target: usize,
    max_ratio: Option<f64>,
    input_covered_ips: u128,
    preferred_set: &PreferredSet,
    max_non_preferred_ratio: Option<f64>,
) -> Vec<NodeIdx> {
    let mut remaining = trie.total_leaf_count();
    // Early exit: already within budget, no optimization needed
    if remaining <= target {
        return trie.extract_leaf_indices();
    }

    // Determine if preferred set has entries for this address family
    let has_preferred = if trie.addr_bits == 32 { !preferred_set.is_empty_v4() } else { !preferred_set.is_empty_v6() };

    let mut heap: BinaryHeap<HeapEntry> = BinaryHeap::new();
    let mut current_over_coverage: u128 = 0;
    let mut current_non_preferred_over_coverage: u128 = 0;

    // Initialize heap with all internal nodes having leaf_count >= 2
    for idx in 0..trie.arena.len() {
        let node = &trie.arena[idx];
        // Skip excluded nodes — they must never be collapsed
        if node.is_excluded {
            continue;
        }
        // Only internal nodes with mergeable children are candidates
        if node.leaf_count >= 2 && !node.is_leaf {
            let cost = trie.collapse_cost(idx as u32);
            let descendant_collapsed = trie.arena[idx].collapsed_cost_sum;
            let net_new = cost.saturating_sub(descendant_collapsed);

            // Discount preferred overlap from effective cost when preferred set is active
            let effective_cost = if has_preferred {
                compute_effective_cost(trie, idx as u32, net_new, preferred_set)
            } else {
                net_new
            };

            let eff = efficiency_key(effective_cost, node.leaf_count);
            heap.push(Reverse((eff, idx as u32, node.generation)));
        }
    }

    // Main greedy loop: pop cheapest merge candidate until budget met or ratio exceeded
    while remaining > target {
        let Some(Reverse((_eff, node_idx, gen))) = heap.pop() else { break };

        // Guard against stale heap entries referencing invalid indices
        if node_idx as usize >= trie.arena.len() {
            continue;
        }

        let node = &trie.arena[node_idx as usize];
        // Skip invalidated, already-collapsed, or stale-generation entries
        if node.is_excluded || node.is_leaf || node.generation != gen {
            // Periodically compact heap to remove stale entries when bloat exceeds 4× useful size.
            // Threshold balances compaction cost (O(n) rebuild) against wasted pop operations.
            if heap.len() > 4 * remaining {
                let arena = &trie.arena;
                // Retain only entries still valid (not collapsed, correct generation)
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

        // Recompute cost from trie state — heap entry may be stale due to descendant
        // collapses that changed the effective coverage since this entry was enqueued.
        let cost = trie.collapse_cost(node_idx);
        let descendant_collapsed_cost = trie.arena[node_idx as usize].collapsed_cost_sum;
        debug_assert!(cost >= descendant_collapsed_cost);
        let net_new_over = cost.saturating_sub(descendant_collapsed_cost);

        // Compute how many preferred IPs fall in the gap (not already covered by leaves)
        let preferred_in_gap = if has_preferred {
            let (start, end) = trie.node_interval(node_idx);
            let total_preferred_in_node = if trie.addr_bits == 32 {
                preferred_set.overlap_count_v4(start, end)
            } else {
                preferred_set.overlap_count_v6(start, end)
            };
            // Preferred IPs in the gap = total preferred in node - preferred already covered by leaves
            total_preferred_in_node.saturating_sub(trie.arena[node_idx as usize].preferred_overlap)
        } else {
            0
        };
        let non_preferred_in_gap = net_new_over.saturating_sub(preferred_in_gap.min(net_new_over));

        // Abort if this merge would push total over-coverage beyond the allowed ratio
        if let Some(max_ratio) = max_ratio {
            if input_covered_ips > 0 {
                let new_total = current_over_coverage.saturating_add(net_new_over);
                if exceeds_ratio(new_total, input_covered_ips, max_ratio) {
                    break;
                }
            }
        }

        // Abort if non-preferred over-coverage would exceed its separate ratio cap
        if let Some(max_np_ratio) = max_non_preferred_ratio {
            if input_covered_ips > 0 {
                let new_np = current_non_preferred_over_coverage.saturating_add(non_preferred_in_gap);
                if exceeds_ratio(new_np, input_covered_ips, max_np_ratio) {
                    break;
                }
            }
        }

        current_over_coverage = current_over_coverage.saturating_add(net_new_over);
        current_non_preferred_over_coverage = current_non_preferred_over_coverage.saturating_add(non_preferred_in_gap);

        // Capture children's preferred_overlap_in_coverage before invalidate_subtree
        // destroys them — these values are needed to preserve the running total.
        let children_poc_sum = {
            let children = trie.arena[node_idx as usize].children;
            children.iter().filter(|&&c| c != INVALID)
                .map(|&c| trie.arena[c as usize].preferred_overlap_in_coverage)
                .sum::<u128>()
        };

        trie.invalidate_subtree(node_idx);
        trie.collapse(node_idx);

        // Preserve accumulated preferred_overlap_in_coverage from children
        trie.arena[node_idx as usize].preferred_overlap_in_coverage = children_poc_sum;

        // Collapsed node now covers full interval; update preferred overlap accordingly
        if has_preferred {
            let (start, end) = trie.node_interval(node_idx);
            let full_preferred = if trie.addr_bits == 32 {
                preferred_set.overlap_count_v4(start, end)
            } else {
                preferred_set.overlap_count_v6(start, end)
            };
            trie.arena[node_idx as usize].preferred_overlap = full_preferred;
        }

        remaining = remaining.saturating_sub(reduction);

        // Propagate updated costs and leaf counts up the ancestor chain
        let mut ancestor = trie.arena[node_idx as usize].parent;
        while ancestor != INVALID {
            trie.update_leaf_count(ancestor);
            trie.arena[ancestor as usize].collapsed_cost_sum = trie.arena[ancestor as usize]
                .collapsed_cost_sum
                .saturating_add(net_new_over);
            // Recompute ancestor's preferred metrics from its children
            if has_preferred {
                let children = trie.arena[ancestor as usize].children;
                let mut total_po: u128 = 0;
                let mut total_poc: u128 = 0;
                for &child in &children {
                    if child != INVALID {
                        total_po = total_po.saturating_add(trie.arena[child as usize].preferred_overlap);
                        total_poc = total_poc.saturating_add(trie.arena[child as usize].preferred_overlap_in_coverage);
                    }
                }
                trie.arena[ancestor as usize].preferred_overlap = total_po;
                trie.arena[ancestor as usize].preferred_overlap_in_coverage = total_poc;
            }
            trie.arena[ancestor as usize].generation =
                trie.arena[ancestor as usize].generation.wrapping_add(1);
            let a = &trie.arena[ancestor as usize];
            // Re-enqueue ancestor as a merge candidate with updated efficiency
            if !a.is_excluded && !a.is_leaf && a.leaf_count >= 2 {
                let anc_cost = trie.collapse_cost(ancestor);
                let anc_descendant = a.collapsed_cost_sum;
                let anc_net_new = anc_cost.saturating_sub(anc_descendant);
                let anc_effective = if has_preferred {
                    compute_effective_cost(trie, ancestor, anc_net_new, preferred_set)
                } else {
                    anc_net_new
                };
                let anc_eff = efficiency_key(anc_effective, a.leaf_count);
                heap.push(Reverse((anc_eff, ancestor, a.generation)));
            }
            ancestor = trie.arena[ancestor as usize].parent;
        }
    }

    trie.extract_leaf_indices()
}

/// Compute effective cost discounting preferred overlap in the gap.
fn compute_effective_cost(trie: &BinaryTrie, node_idx: NodeIdx, net_new_over: u128, preferred_set: &PreferredSet) -> u128 {
    let (start, end) = trie.node_interval(node_idx);
    let total_preferred_in_node = if trie.addr_bits == 32 {
        preferred_set.overlap_count_v4(start, end)
    } else {
        preferred_set.overlap_count_v6(start, end)
    };
    let preferred_in_gap = total_preferred_in_node.saturating_sub(trie.arena[node_idx as usize].preferred_overlap);
    net_new_over.saturating_sub(preferred_in_gap.min(net_new_over))
}

/// Integer-scaled ratio check for overflow safety with large IPv6 values.
pub fn exceeds_ratio(over: u128, covered: u128, max_ratio: f64) -> bool {
    if over <= u64::MAX as u128 && covered <= u64::MAX as u128 {
        return (over as f64) > max_ratio * (covered as f64);
    }
    // SCALE=1M gives 6 decimal digits of precision without overflowing most u128 products
    const SCALE: u128 = 1_000_000;
    let threshold = (max_ratio * SCALE as f64) as u128;
    match over.checked_mul(SCALE) {
        Some(scaled_over) => match threshold.checked_mul(covered) {
            // None from checked_mul means threshold*covered overflowed u128 — ratio is
            // astronomically large, so over cannot exceed it; conservatively return false
            Some(rhs) => scaled_over > rhs,
            None => false,
        },
        // over*SCALE overflowed: fall back to f64 division which is acceptable here because
        // we only reach this branch for extremely large values where precision loss is negligible
        None => (over as f64 / covered as f64) > max_ratio,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lossless::SourceMapPrefix;
    use crate::preferred::PreferredSet;

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
        assert!(!exceeds_ratio(0, 0, 0.05));
    }

    #[test]
    fn optimize_small_trie_to_target() {
        let entries = vec![
            SourceMapPrefix { prefix: "10.0.0.0/25".parse().unwrap(), source_indices: vec![0], coverage: 128, preferred_overlap_in_coverage: 0 },
            SourceMapPrefix { prefix: "10.0.0.128/25".parse().unwrap(), source_indices: vec![1], coverage: 128, preferred_overlap_in_coverage: 0 },
            SourceMapPrefix { prefix: "10.0.1.0/25".parse().unwrap(), source_indices: vec![2], coverage: 128, preferred_overlap_in_coverage: 0 },
            SourceMapPrefix { prefix: "10.0.1.128/25".parse().unwrap(), source_indices: vec![3], coverage: 128, preferred_overlap_in_coverage: 0 },
        ];
        let mut trie = BinaryTrie::build_from_v4(&entries).unwrap();
        assert_eq!(trie.total_leaf_count(), 4);

        let empty_pref = PreferredSet::build(&[]);
        let _leaves = optimize_trie(&mut trie, 2, None, 512, &empty_pref, None);
        assert!(trie.total_leaf_count() <= 2);
    }

    #[test]
    fn optimize_target_already_met() {
        let entries = vec![
            SourceMapPrefix { prefix: "10.0.0.0/24".parse().unwrap(), source_indices: vec![0], coverage: 256, preferred_overlap_in_coverage: 0 },
            SourceMapPrefix { prefix: "10.0.1.0/24".parse().unwrap(), source_indices: vec![1], coverage: 256, preferred_overlap_in_coverage: 0 },
        ];
        let mut trie = BinaryTrie::build_from_v4(&entries).unwrap();
        let empty_pref = PreferredSet::build(&[]);
        let _leaves = optimize_trie(&mut trie, 5, None, 512, &empty_pref, None);
        assert_eq!(trie.total_leaf_count(), 2);
    }

    #[test]
    fn optimize_with_ratio_cap() {
        let entries = vec![
            SourceMapPrefix { prefix: "10.0.0.1/32".parse().unwrap(), source_indices: vec![0], coverage: 1, preferred_overlap_in_coverage: 0 },
            SourceMapPrefix { prefix: "10.0.0.2/32".parse().unwrap(), source_indices: vec![1], coverage: 1, preferred_overlap_in_coverage: 0 },
            SourceMapPrefix { prefix: "192.168.0.1/32".parse().unwrap(), source_indices: vec![2], coverage: 1, preferred_overlap_in_coverage: 0 },
            SourceMapPrefix { prefix: "192.168.0.2/32".parse().unwrap(), source_indices: vec![3], coverage: 1, preferred_overlap_in_coverage: 0 },
        ];
        let mut trie = BinaryTrie::build_from_v4(&entries).unwrap();
        let empty_pref = PreferredSet::build(&[]);
        let _leaves = optimize_trie(&mut trie, 1, Some(0.0), 4, &empty_pref, None);
        assert_eq!(trie.total_leaf_count(), 4);
    }

    #[test]
    fn widening_mul_basic() {
        assert_eq!(widening_mul_u128_u32(1, 1), (0, 1));
        assert_eq!(widening_mul_u128_u32(u128::MAX, 1), (0, u128::MAX));
        let (hi, lo) = widening_mul_u128_u32(u128::MAX, 2);
        assert_eq!(hi, 1);
        assert_eq!(lo, u128::MAX - 1);
    }

    #[test]
    fn optimize_respects_exclusion() {
        use crate::exclusion::ExclusionSet;
        use crate::trie::BinaryTrie;
        use crate::types::ExclusionEntry;

        let entries = vec![
            SourceMapPrefix { prefix: "10.0.0.0/24".parse().unwrap(), source_indices: vec![0], coverage: 256, preferred_overlap_in_coverage: 0 },
            SourceMapPrefix { prefix: "10.0.2.0/24".parse().unwrap(), source_indices: vec![1], coverage: 256, preferred_overlap_in_coverage: 0 },
        ];

        // Without exclusion: target=1 merges to /22
        let mut trie_no_excl = BinaryTrie::build_from_v4(&entries).unwrap();
        let empty_pref = PreferredSet::build(&[]);
        let _leaves = optimize_trie(&mut trie_no_excl, 1, None, 512, &empty_pref, None);
        assert_eq!(trie_no_excl.total_leaf_count(), 1);

        // With exclusion of 10.0.1.0/24 (in the /22 gap, not covered by input)
        let mut trie = BinaryTrie::build_from_v4(&entries).unwrap();
        let excl = vec![ExclusionEntry {
            prefix: "10.0.1.0/24".parse().unwrap(),
            source: "test".to_string(),
            comment: None,
        }];
        let set = ExclusionSet::build(&excl);
        trie.mark_exclusions(&set, true);

        let _leaves = optimize_trie(&mut trie, 1, None, 512, &empty_pref, None);
        assert_eq!(trie.total_leaf_count(), 2);
    }

    #[test]
    fn optimize_with_preferred_biases_merge() {
        use crate::types::PreferredEntry;

        // Two /24s with a gap: 10.0.0.0/24 and 10.0.2.0/24
        // Merge to /22 introduces 512 IPs over-coverage (10.0.1.0/24 + 10.0.3.0/24). Preferred covers 10.0.1.0/24.
        let entries = vec![
            SourceMapPrefix { prefix: "10.0.0.0/24".parse().unwrap(), source_indices: vec![0], coverage: 256, preferred_overlap_in_coverage: 0 },
            SourceMapPrefix { prefix: "10.0.2.0/24".parse().unwrap(), source_indices: vec![1], coverage: 256, preferred_overlap_in_coverage: 0 },
        ];

        let preferred = vec![PreferredEntry {
            prefix: "10.0.1.0/24".parse().unwrap(),
            source: "test".into(),
            comment: None,
        }];
        let pref_set = PreferredSet::build(&preferred);

        let mut trie = BinaryTrie::build_from_v4(&entries).unwrap();
        trie.mark_preferred_overlaps(&pref_set, true);
        // max_coverage=512: allows the full /22 merge (512 IPs of over-coverage)
        let _leaves = optimize_trie(&mut trie, 1, None, 512, &pref_set, None);
        // Should merge since preferred covers the gap
        assert_eq!(trie.total_leaf_count(), 1);
    }

    #[test]
    fn optimize_max_non_preferred_ratio_blocks() {
        use crate::types::PreferredEntry;

        // Two /24s: 10.0.0.0/24 and 10.0.2.0/24. Gap = 10.0.1.0/24 + 10.0.3.0/24 at /22 level.
        // Preferred covers 10.0.1.0/24 but not 10.0.3.0/24.
        // With max_non_preferred_ratio=0, merging to /22 should be blocked (non-preferred gap exists).
        let entries = vec![
            SourceMapPrefix { prefix: "10.0.0.0/24".parse().unwrap(), source_indices: vec![0], coverage: 256, preferred_overlap_in_coverage: 0 },
            SourceMapPrefix { prefix: "10.0.2.0/24".parse().unwrap(), source_indices: vec![1], coverage: 256, preferred_overlap_in_coverage: 0 },
        ];

        let preferred = vec![PreferredEntry {
            prefix: "10.0.1.0/24".parse().unwrap(),
            source: "test".into(),
            comment: None,
        }];
        let pref_set = PreferredSet::build(&preferred);

        let mut trie = BinaryTrie::build_from_v4(&entries).unwrap();
        trie.mark_preferred_overlaps(&pref_set, true);
        // max_non_preferred_ratio=0 means no non-preferred over-coverage allowed
        // max_coverage=512: would allow /22 merge if not blocked by non-preferred ratio
        let _leaves = optimize_trie(&mut trie, 1, None, 512, &pref_set, Some(0.0));
        // /22 merge would absorb 10.0.3.0/24 (not preferred) → blocked by ratio cap.
        // Neither /23 parent helps: each input is alone in its /23 subtree.
        // Result: both leaves remain, target=1 cannot be reached.
        assert_eq!(trie.total_leaf_count(), 2);
    }
}
