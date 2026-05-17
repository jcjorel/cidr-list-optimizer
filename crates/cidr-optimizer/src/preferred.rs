//! Preferred-entry overlap detection using sorted interval sets with binary-search pruning.

use ipnet::{IpNet, Ipv4Net, Ipv6Net};

use crate::lossless;
use crate::lossless::SourceMapPrefix;
use crate::types::PreferredEntry;

/// Sorted non-overlapping intervals (post-aggregation) for binary-search-pruned overlap queries.
pub struct PreferredSet {
    ipv4_ranges: Vec<(u128, u128)>,
    ipv6_ranges: Vec<(u128, u128)>,
}

impl PreferredSet {
    /// Build from preferred entries, applying lossless aggregation to produce non-overlapping
    /// intervals that prevent double-counting in overlap queries.
    pub fn build(entries: &[PreferredEntry]) -> Self {
        let mut v4_input: Vec<(usize, Ipv4Net)> = Vec::new();
        let mut v6_input: Vec<(usize, Ipv6Net)> = Vec::new();

        // Partition entries by address family for separate aggregation
        for (i, entry) in entries.iter().enumerate() {
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

    /// Count how many IPs in [candidate_start, candidate_end] overlap with preferred IPv4 ranges.
    pub fn overlap_count_v4(&self, candidate_start: u128, candidate_end: u128) -> u128 {
        Self::overlap_count(&self.ipv4_ranges, candidate_start, candidate_end)
    }

    /// Count how many IPs in [candidate_start, candidate_end] overlap with preferred IPv6 ranges.
    pub fn overlap_count_v6(&self, candidate_start: u128, candidate_end: u128) -> u128 {
        Self::overlap_count(&self.ipv6_ranges, candidate_start, candidate_end)
    }

    /// Find original preferred entries whose prefix intersects the candidate IPv4 interval.
    ///
    /// Uses a linear scan of original entries (not the pre-built interval index) because
    /// callers need references to `PreferredEntry` structs with source/comment metadata
    /// that the sorted interval vectors discard.
    pub fn find_overlapping_v4<'a>(
        &self,
        entries: &'a [PreferredEntry],
        candidate_start: u128,
        candidate_end: u128,
    ) -> Vec<&'a PreferredEntry> {
        Self::find_overlapping(entries, candidate_start, candidate_end, true)
    }

    /// Find original preferred entries whose prefix intersects the candidate IPv6 interval.
    ///
    /// Uses a linear scan of original entries (not the pre-built interval index) because
    /// callers need references to `PreferredEntry` structs with source/comment metadata
    /// that the sorted interval vectors discard.
    pub fn find_overlapping_v6<'a>(
        &self,
        entries: &'a [PreferredEntry],
        candidate_start: u128,
        candidate_end: u128,
    ) -> Vec<&'a PreferredEntry> {
        Self::find_overlapping(entries, candidate_start, candidate_end, false)
    }

    /// Returns true if no IPv4 preferred ranges exist.
    pub fn is_empty_v4(&self) -> bool {
        self.ipv4_ranges.is_empty()
    }

    /// Returns true if no IPv6 preferred ranges exist.
    pub fn is_empty_v6(&self) -> bool {
        self.ipv6_ranges.is_empty()
    }

    /// Sum the number of IPs in [candidate_start, candidate_end] that fall within any stored interval.
    fn overlap_count(ranges: &[(u128, u128)], candidate_start: u128, candidate_end: u128) -> u128 {
        // Uses partition_point for binary search to find the upper bound of potentially
        // overlapping intervals, then linearly scans those intervals to sum overlap.
        if ranges.is_empty() {
            return 0;
        }
        let upper = ranges.partition_point(|&(start, _)| start <= candidate_end);
        let mut total: u128 = 0;
        for &(start, end) in &ranges[..upper] {
            // Skip intervals that end before the candidate starts
            if end < candidate_start {
                continue;
            }
            let overlap_start = start.max(candidate_start);
            let overlap_end = end.min(candidate_end);
            total = total.saturating_add(overlap_end - overlap_start + 1);
        }
        total
    }

    /// Linear scan of entries to find those whose prefix intersects [candidate_start, candidate_end].
    /// Uses original entries (not the pre-built index) to return references to PreferredEntry structs.
    fn find_overlapping(
        entries: &[PreferredEntry],
        candidate_start: u128,
        candidate_end: u128,
        is_v4: bool,
    ) -> Vec<&PreferredEntry> {
        entries
            .iter()
            .filter(|e| {
                // Compute start/end for the entry's address family, skip mismatched families
                let (start, end) = match &e.prefix {
                    IpNet::V4(v4) if is_v4 => {
                        (u32::from(v4.network()) as u128, u32::from(v4.broadcast()) as u128)
                    }
                    IpNet::V6(v6) if !is_v4 => {
                        let s = u128::from(v6.network());
                        let pl = v6.prefix_len();
                        // Compute broadcast equivalent: set all host bits to 1
                        let e = if pl == 128 { s } else if pl == 0 { u128::MAX } else { s | ((1u128 << (128 - pl)) - 1) };
                        (s, e)
                    }
                    _ => return false,
                };
                start <= candidate_end && end >= candidate_start
            })
            .collect()
    }

    /// Convert aggregated IPv4 prefixes into sorted numeric intervals for binary search.
    fn to_intervals_v4(agg: &[SourceMapPrefix<Ipv4Net>]) -> Vec<(u128, u128)> {
        // Numeric intervals enable binary-search-based overlap queries
        let mut intervals: Vec<(u128, u128)> = agg
            .iter()
            .map(|e| {
                let start = u32::from(e.prefix.network()) as u128;
                let end = u32::from(e.prefix.broadcast()) as u128;
                (start, end)
            })
            .collect();
        // Sorted order is required for partition_point binary search in overlap_count
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
                // Handle edge cases: /128 is a single address, /0 spans the entire address space
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_set_returns_zero() {
        let set = PreferredSet::build(&[]);
        assert_eq!(set.overlap_count_v4(0, 255), 0);
        assert!(set.is_empty_v4());
        assert!(set.is_empty_v6());
    }

    #[test]
    fn single_range_fully_inside() {
        // Preferred: 10.0.0.0/24 (256 IPs). Candidate covers it entirely.
        let entries = vec![PreferredEntry {
            prefix: "10.0.0.0/24".parse().unwrap(),
            source: "test".into(),
            comment: None,
        }];
        let set = PreferredSet::build(&entries);
        let start: u128 = u32::from("10.0.0.0".parse::<std::net::Ipv4Addr>().unwrap()) as u128;
        let end: u128 = u32::from("10.0.1.255".parse::<std::net::Ipv4Addr>().unwrap()) as u128;
        assert_eq!(set.overlap_count_v4(start, end), 256);
    }

    #[test]
    fn partial_overlap() {
        // Preferred: 10.0.0.0/24. Candidate: 10.0.0.128-10.0.1.255 (partial overlap = 128 IPs)
        let entries = vec![PreferredEntry {
            prefix: "10.0.0.0/24".parse().unwrap(),
            source: "test".into(),
            comment: None,
        }];
        let set = PreferredSet::build(&entries);
        let start: u128 = u32::from("10.0.0.128".parse::<std::net::Ipv4Addr>().unwrap()) as u128;
        let end: u128 = u32::from("10.0.1.255".parse::<std::net::Ipv4Addr>().unwrap()) as u128;
        assert_eq!(set.overlap_count_v4(start, end), 128);
    }

    #[test]
    fn multiple_disjoint_ranges() {
        let entries = vec![
            PreferredEntry { prefix: "10.0.0.0/24".parse().unwrap(), source: "a".into(), comment: None },
            PreferredEntry { prefix: "10.0.2.0/24".parse().unwrap(), source: "b".into(), comment: None },
        ];
        let set = PreferredSet::build(&entries);
        // Candidate covers both: 10.0.0.0 - 10.0.2.255
        let start: u128 = u32::from("10.0.0.0".parse::<std::net::Ipv4Addr>().unwrap()) as u128;
        let end: u128 = u32::from("10.0.2.255".parse::<std::net::Ipv4Addr>().unwrap()) as u128;
        assert_eq!(set.overlap_count_v4(start, end), 512);
    }

    #[test]
    fn no_overlap() {
        let entries = vec![PreferredEntry {
            prefix: "10.0.0.0/24".parse().unwrap(),
            source: "test".into(),
            comment: None,
        }];
        let set = PreferredSet::build(&entries);
        let start: u128 = u32::from("192.168.0.0".parse::<std::net::Ipv4Addr>().unwrap()) as u128;
        let end: u128 = u32::from("192.168.0.255".parse::<std::net::Ipv4Addr>().unwrap()) as u128;
        assert_eq!(set.overlap_count_v4(start, end), 0);
    }

    #[test]
    fn find_overlapping_returns_correct_entries() {
        let entries = vec![
            PreferredEntry { prefix: "10.0.0.0/24".parse().unwrap(), source: "file1".into(), comment: Some("pool A".into()) },
            PreferredEntry { prefix: "10.0.2.0/24".parse().unwrap(), source: "file1".into(), comment: Some("pool B".into()) },
        ];
        let set = PreferredSet::build(&entries);
        let start: u128 = u32::from("10.0.0.0".parse::<std::net::Ipv4Addr>().unwrap()) as u128;
        let end: u128 = u32::from("10.0.0.255".parse::<std::net::Ipv4Addr>().unwrap()) as u128;
        let found = set.find_overlapping_v4(&entries, start, end);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].comment, Some("pool A".into()));
    }

    #[test]
    fn ipv6_overlap_count() {
        let entries = vec![PreferredEntry {
            prefix: "2001:db8::/48".parse().unwrap(),
            source: "test".into(),
            comment: None,
        }];
        let set = PreferredSet::build(&entries);
        assert!(!set.is_empty_v6());
        let start = u128::from("2001:db8::".parse::<std::net::Ipv6Addr>().unwrap());
        let end = start | ((1u128 << 80) - 1);
        // /48 has 2^80 IPs
        assert_eq!(set.overlap_count_v6(start, end), 1u128 << 80);
    }

    #[test]
    fn overlapping_preferred_entries_deduplicated() {
        // 10.0.0.0/24 and 10.0.0.0/25 overlap — lossless aggregation deduplicates to /24.
        let entries = vec![
            PreferredEntry { prefix: "10.0.0.0/24".parse().unwrap(), source: "a".into(), comment: None },
            PreferredEntry { prefix: "10.0.0.0/25".parse().unwrap(), source: "b".into(), comment: None },
        ];
        let set = PreferredSet::build(&entries);
        let start: u128 = u32::from("10.0.0.0".parse::<std::net::Ipv4Addr>().unwrap()) as u128;
        let end: u128 = u32::from("10.0.0.255".parse::<std::net::Ipv4Addr>().unwrap()) as u128;
        // Must count 256, not 384 (no double-counting from overlap)
        assert_eq!(set.overlap_count_v4(start, end), 256);
    }
}
