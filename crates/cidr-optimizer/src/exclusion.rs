//! Exclusion zone support for the CIDR optimizer.
//!
//! Provides [`ExclusionSet`] which maintains sorted, non-overlapping intervals
//! for binary-search-pruned intersection queries, preventing protected CIDR ranges from
//! being absorbed by widened supernets during budget-constrained optimization.

use ipnet::{IpNet, Ipv4Net, Ipv6Net};

use crate::lossless;
use crate::lossless::SourceMapPrefix;
use crate::types::ExclusionEntry;

/// Sorted non-overlapping intervals (post-aggregation) for binary-search-pruned intersection queries.
pub struct ExclusionSet {
    /// IPv4 intervals as (start, end) inclusive, sorted by start.
    ipv4_ranges: Vec<(u128, u128)>,
    /// IPv6 intervals as (start, end) inclusive, sorted by start.
    ipv6_ranges: Vec<(u128, u128)>,
}

impl ExclusionSet {
    /// Build from exclusion entries. Losslessly aggregates each address family
    /// to produce minimal non-overlapping intervals.
    pub fn build(entries: &[ExclusionEntry]) -> Self {
        let mut v4_input: Vec<(usize, Ipv4Net)> = Vec::new();
        let mut v6_input: Vec<(usize, Ipv6Net)> = Vec::new();

        // Each address family is aggregated independently because IPv4 and IPv6 intervals are incomparable
        for (i, entry) in entries.iter().enumerate() {
            // Truncate to canonical network address — required for correct lossless aggregation
            match entry.prefix {
                IpNet::V4(v4) => v4_input.push((i, v4.trunc())),
                IpNet::V6(v6) => v6_input.push((i, v6.trunc())),
            }
        }

        let agg_v4 = lossless::lossless_aggregate_v4(v4_input, 32);
        let agg_v6 = lossless::lossless_aggregate_v6(v6_input, 128);

        let ipv4_ranges = Self::to_intervals_v4(&agg_v4);
        let ipv6_ranges = Self::to_intervals_v6(&agg_v6);

        Self { ipv4_ranges, ipv6_ranges }
    }

    /// Returns true if the candidate interval intersects any IPv4 exclusion range.
    pub fn intersects_v4(&self, candidate_start: u128, candidate_end: u128) -> bool {
        Self::intersects_ranges(&self.ipv4_ranges, candidate_start, candidate_end)
    }

    /// Returns true if the candidate interval intersects any IPv6 exclusion range.
    pub fn intersects_v6(&self, candidate_start: u128, candidate_end: u128) -> bool {
        Self::intersects_ranges(&self.ipv6_ranges, candidate_start, candidate_end)
    }

    /// Returns true if the exclusion set has no IPv4 entries.
    pub fn is_empty_v4(&self) -> bool {
        self.ipv4_ranges.is_empty()
    }

    /// Returns true if the exclusion set has no IPv6 entries.
    pub fn is_empty_v6(&self) -> bool {
        self.ipv6_ranges.is_empty()
    }

    /// Find all original exclusion entries that intersect the given IPv4 interval.
    pub fn find_intersecting_v4<'a>(
        &self,
        entries: &'a [ExclusionEntry],
        candidate_start: u128,
        candidate_end: u128,
    ) -> Vec<&'a ExclusionEntry> {
        Self::find_intersecting(entries, candidate_start, candidate_end, true)
    }

    /// Find all original exclusion entries that intersect the given IPv6 interval.
    pub fn find_intersecting_v6<'a>(
        &self,
        entries: &'a [ExclusionEntry],
        candidate_start: u128,
        candidate_end: u128,
    ) -> Vec<&'a ExclusionEntry> {
        Self::find_intersecting(entries, candidate_start, candidate_end, false)
    }

    /// Precondition: ranges are sorted and non-overlapping. Find the last range starting
    /// at or before candidate_end; overlap exists iff that range extends into the candidate interval.
    fn intersects_ranges(ranges: &[(u128, u128)], candidate_start: u128, candidate_end: u128) -> bool {
        let idx = ranges.partition_point(|&(start, _)| start <= candidate_end);
        idx > 0 && ranges[idx - 1].1 >= candidate_start
    }

    /// Convert aggregated IPv4 prefixes into sorted numeric intervals for binary search.
    fn to_intervals_v4(agg: &[SourceMapPrefix<Ipv4Net>]) -> Vec<(u128, u128)> {
        // Numeric intervals enable O(log E) binary-search intersection checks
        let mut intervals: Vec<(u128, u128)> = agg
            .iter()
            .map(|e| {
                let start = u32::from(e.prefix.network()) as u128;
                let end = u32::from(e.prefix.broadcast()) as u128;
                (start, end)
            })
            .collect();
        // Sort by start address — required for binary search in intersects_ranges
        intervals.sort_unstable_by_key(|&(s, _)| s);
        intervals
    }

    /// Convert aggregated IPv6 prefixes into sorted numeric intervals for binary search.
    fn to_intervals_v6(agg: &[SourceMapPrefix<Ipv6Net>]) -> Vec<(u128, u128)> {
        // Same approach as to_intervals_v4 but with 128-bit address arithmetic
        let mut intervals: Vec<(u128, u128)> = agg
            .iter()
            .map(|e| {
                let start = u128::from(e.prefix.network());
                let pl = e.prefix.prefix_len();
                // Compute last address: /128 is a single host, /0 spans all addresses,
                // general case uses bitmask (special-cased to avoid 1u128 << 128 overflow)
                let end = if pl == 128 {
                    start
                } else if pl == 0 {
                    u128::MAX
                } else {
                    start | ((1u128 << (128 - pl)) - 1)
                };
                (start, end)
            })
            .collect();
        intervals.sort_unstable_by_key(|&(s, _)| s);
        intervals
    }

    /// Linear scan over entries to collect those intersecting the candidate range.
    /// Unlike `intersects_ranges` (which only returns bool), this needs to return
    /// references to the original entries, which aren't stored in the sorted interval vec.
    fn find_intersecting(
        entries: &[ExclusionEntry],
        candidate_start: u128,
        candidate_end: u128,
        is_v4: bool,
    ) -> Vec<&ExclusionEntry> {
        entries
            .iter()
            .filter(|e| {
                let (start, end) = match &e.prefix {
                    IpNet::V4(v4) if is_v4 => {
                        (u32::from(v4.network()) as u128, u32::from(v4.broadcast()) as u128)
                    }
                    IpNet::V6(v6) if !is_v4 => {
                        let s = u128::from(v6.network());
                        let pl = v6.prefix_len();
                        // Last address via bitmask — special cases avoid 1u128 << 128 overflow (mirrors to_intervals_v6)
                        let e = if pl == 128 { s } else if pl == 0 { u128::MAX } else { s | ((1u128 << (128 - pl)) - 1) };
                        (s, e)
                    }
                    _ => return false,
                };
                start <= candidate_end && end >= candidate_start
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_exclusion_set() {
        let set = ExclusionSet::build(&[]);
        assert!(set.is_empty_v4());
        assert!(set.is_empty_v6());
        assert!(!set.intersects_v4(0, 255));
    }

    #[test]
    fn intersects_v4_basic() {
        let entries = vec![ExclusionEntry {
            prefix: "10.0.0.0/24".parse().unwrap(),
            source: "test".to_string(),
            comment: None,
        }];
        let set = ExclusionSet::build(&entries);
        // 10.0.0.0/24 = 167772160..167772415
        let start: u128 = u32::from("10.0.0.0".parse::<std::net::Ipv4Addr>().unwrap()) as u128;
        let end: u128 = u32::from("10.0.0.255".parse::<std::net::Ipv4Addr>().unwrap()) as u128;

        // Exact match
        assert!(set.intersects_v4(start, end));
        // Subset
        assert!(set.intersects_v4(start, start + 10));
        // Superset
        assert!(set.intersects_v4(start - 1, end + 1));
        // Non-overlapping (before)
        assert!(!set.intersects_v4(0, start - 1));
        // Non-overlapping (after)
        assert!(!set.intersects_v4(end + 1, end + 100));
    }

    #[test]
    fn intersects_v4_multiple_ranges() {
        let entries = vec![
            ExclusionEntry {
                prefix: "10.0.0.0/24".parse().unwrap(),
                source: "a".to_string(),
                comment: None,
            },
            ExclusionEntry {
                prefix: "192.168.1.0/24".parse().unwrap(),
                source: "b".to_string(),
                comment: None,
            },
        ];
        let set = ExclusionSet::build(&entries);

        let start_192: u128 = u32::from("192.168.1.0".parse::<std::net::Ipv4Addr>().unwrap()) as u128;
        assert!(set.intersects_v4(start_192, start_192 + 255));

        // Gap between the two ranges
        let gap_start: u128 = u32::from("10.0.1.0".parse::<std::net::Ipv4Addr>().unwrap()) as u128;
        let gap_end: u128 = u32::from("192.168.0.255".parse::<std::net::Ipv4Addr>().unwrap()) as u128;
        assert!(!set.intersects_v4(gap_start, gap_end));
    }

    #[test]
    fn intersects_v6_basic() {
        let entries = vec![ExclusionEntry {
            prefix: "2001:db8::/32".parse().unwrap(),
            source: "test".to_string(),
            comment: None,
        }];
        let set = ExclusionSet::build(&entries);
        assert!(!set.is_empty_v6());

        let start = u128::from("2001:db8::".parse::<std::net::Ipv6Addr>().unwrap());
        let end = start | ((1u128 << 96) - 1);
        assert!(set.intersects_v6(start, end));
        assert!(!set.intersects_v6(0, start - 1));
    }

    #[test]
    fn find_intersecting_returns_matching_entries() {
        let entries = vec![
            ExclusionEntry {
                prefix: "10.0.0.0/24".parse().unwrap(),
                source: "file1".to_string(),
                comment: Some("pool A".to_string()),
            },
            ExclusionEntry {
                prefix: "10.0.1.0/24".parse().unwrap(),
                source: "file1".to_string(),
                comment: Some("pool B".to_string()),
            },
        ];
        let set = ExclusionSet::build(&entries);

        let start: u128 = u32::from("10.0.0.0".parse::<std::net::Ipv4Addr>().unwrap()) as u128;
        let end: u128 = u32::from("10.0.0.255".parse::<std::net::Ipv4Addr>().unwrap()) as u128;

        let found = set.find_intersecting_v4(&entries, start, end);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].comment, Some("pool A".to_string()));
    }

    #[test]
    fn adjacent_exclusions_are_aggregated() {
        // Two adjacent /25s should aggregate to a single /24 interval
        let entries = vec![
            ExclusionEntry {
                prefix: "10.0.0.0/25".parse().unwrap(),
                source: "test".to_string(),
                comment: None,
            },
            ExclusionEntry {
                prefix: "10.0.0.128/25".parse().unwrap(),
                source: "test".to_string(),
                comment: None,
            },
        ];
        let set = ExclusionSet::build(&entries);
        // The aggregated interval should cover the full /24
        let start: u128 = u32::from("10.0.0.0".parse::<std::net::Ipv4Addr>().unwrap()) as u128;
        let end: u128 = u32::from("10.0.0.255".parse::<std::net::Ipv4Addr>().unwrap()) as u128;
        assert!(set.intersects_v4(start, end));
        // Only one interval after aggregation
        assert_eq!(set.ipv4_ranges.len(), 1);
    }
}
