use ipnet::{Ipv4Net, Ipv6Net};
use std::net::{Ipv4Addr, Ipv6Addr};

/// A prefix with source-map tracking and coverage information.
#[derive(Debug, Clone)]
pub struct SourceMapPrefix<N> {
    pub prefix: N,
    pub source_indices: Vec<usize>,
    /// Number of IPs actually covered by original inputs within this prefix.
    /// For lossless entries, equals the prefix capacity (zero over-coverage).
    pub coverage: u128,
    /// Number of preferred IPs overlapping with the covered portion of this prefix.
    /// Populated by the trie during lossy optimization; 0 for lossless-only entries.
    pub preferred_overlap_in_coverage: u128,
}

/// Lossless aggregation for IPv4 prefixes.
pub fn lossless_aggregate_v4(
    input: Vec<(usize, Ipv4Net)>,
    max_prefix_len: u8,
) -> Vec<SourceMapPrefix<Ipv4Net>> {
    if input.is_empty() {
        return Vec::new();
    }

    // Build source-map entries
    let mut entries: Vec<SourceMapPrefix<Ipv4Net>> = input
        .into_iter()
        .map(|(idx, prefix)| {
            let pl = prefix.prefix_len();
            let cap = if pl == 32 { 1u128 } else { 1u128 << (32 - pl) };
            SourceMapPrefix {
                prefix,
                source_indices: vec![idx],
                coverage: cap,
                preferred_overlap_in_coverage: 0,
            }
        })
        .collect();

    // Enforce max_prefix_len: truncate longer prefixes
    if max_prefix_len < 32 {
        for entry in &mut entries {
            if entry.prefix.prefix_len() > max_prefix_len {
                entry.prefix = Ipv4Net::new(
                    trunc_v4(entry.prefix.network(), max_prefix_len),
                    max_prefix_len,
                )
                .unwrap();
                // Recalculate coverage to match new prefix capacity
                let cap = 1u128 << (32 - max_prefix_len);
                entry.coverage = cap;
            }
        }
    }

    // Radix sort by (network_address, prefix_length)
    radix_sort_v4(&mut entries);

    // Redundancy elimination
    entries = redundancy_eliminate_v4(entries);

    // Sibling merging
    sibling_merge_v4(&mut entries);

    entries
}

/// Lossless aggregation for IPv6 prefixes.
pub fn lossless_aggregate_v6(
    input: Vec<(usize, Ipv6Net)>,
    max_prefix_len: u8,
) -> Vec<SourceMapPrefix<Ipv6Net>> {
    if input.is_empty() {
        return Vec::new();
    }

    let mut entries: Vec<SourceMapPrefix<Ipv6Net>> = input
        .into_iter()
        .map(|(idx, prefix)| {
            let pl = prefix.prefix_len();
            let cap = if pl == 128 { 1u128 } else if (128 - pl) >= 128 { u128::MAX } else { 1u128 << (128 - pl) };
            SourceMapPrefix {
                prefix,
                source_indices: vec![idx],
                coverage: cap,
                preferred_overlap_in_coverage: 0,
            }
        })
        .collect();

    if max_prefix_len < 128 {
        for entry in &mut entries {
            if entry.prefix.prefix_len() > max_prefix_len {
                entry.prefix = Ipv6Net::new(
                    trunc_v6(entry.prefix.network(), max_prefix_len),
                    max_prefix_len,
                )
                .unwrap();
                // Recalculate coverage to match new prefix capacity
                let shift = 128 - max_prefix_len;
                entry.coverage = if shift >= 128 { u128::MAX } else { 1u128 << shift };
            }
        }
    }

    radix_sort_v6(&mut entries);
    entries = redundancy_eliminate_v6(entries);
    sibling_merge_v6(&mut entries);

    entries
}

// --- IPv4 helpers ---

fn trunc_v4(addr: Ipv4Addr, prefix_len: u8) -> Ipv4Addr {
    let bits = u32::from(addr);
    let mask = if prefix_len == 0 { 0 } else { !0u32 << (32 - prefix_len) };
    Ipv4Addr::from(bits & mask)
}

/// Radix sort by (network_address bytes [0..3], prefix_len) — 5 passes LSD.
fn radix_sort_v4(entries: &mut Vec<SourceMapPrefix<Ipv4Net>>) {
    let len = entries.len();
    if len <= 1 {
        return;
    }
    let mut buf = Vec::with_capacity(len);
    buf.resize_with(len, || SourceMapPrefix {
        prefix: Ipv4Net::new(Ipv4Addr::UNSPECIFIED, 0).unwrap(),
        source_indices: Vec::new(),
        coverage: 0,
        preferred_overlap_in_coverage: 0,
    });

    // Key extraction: 5 bytes = [prefix_len, addr[3], addr[2], addr[1], addr[0]]
    // LSD: sort by least significant byte first
    let key_bytes: Vec<[u8; 5]> = entries
        .iter()
        .map(|e| {
            let octets = e.prefix.network().octets();
            [e.prefix.prefix_len(), octets[3], octets[2], octets[1], octets[0]]
        })
        .collect();

    let mut indices: Vec<usize> = (0..len).collect();
    let mut indices_buf: Vec<usize> = vec![0; len];

    for pass in 0..5 {
        let mut counts = [0u32; 256];
        for &i in &indices {
            counts[key_bytes[i][pass] as usize] += 1;
        }
        let mut offsets = [0u32; 256];
        for i in 1..256 {
            offsets[i] = offsets[i - 1] + counts[i - 1];
        }
        for &i in &indices {
            let byte = key_bytes[i][pass] as usize;
            indices_buf[offsets[byte] as usize] = i;
            offsets[byte] += 1;
        }
        std::mem::swap(&mut indices, &mut indices_buf);
    }

    // Apply permutation
    let mut sorted = Vec::with_capacity(len);
    for &i in &indices {
        sorted.push(std::mem::replace(
            &mut entries[i],
            SourceMapPrefix {
                prefix: Ipv4Net::new(Ipv4Addr::UNSPECIFIED, 0).unwrap(),
                source_indices: Vec::new(),
                coverage: 0,
                preferred_overlap_in_coverage: 0,
            },
        ));
    }
    *entries = sorted;
}

/// Redundancy elimination using monotone stack.
/// Input must be sorted by (network_address, prefix_length).
fn redundancy_eliminate_v4(
    entries: Vec<SourceMapPrefix<Ipv4Net>>,
) -> Vec<SourceMapPrefix<Ipv4Net>> {
    let mut stack: Vec<SourceMapPrefix<Ipv4Net>> = Vec::new();
    let mut output: Vec<SourceMapPrefix<Ipv4Net>> = Vec::new();

    for entry in entries {
        // Pop entries that don't contain the current entry
        while let Some(top) = stack.last() {
            if contains_v4(&top.prefix, &entry.prefix) {
                break;
            }
            output.push(stack.pop().unwrap());
        }

        if let Some(top) = stack.last_mut() {
            if contains_v4(&top.prefix, &entry.prefix) {
                // Current entry is redundant — merge source-map into container
                top.source_indices.extend(&entry.source_indices);
                // Coverage doesn't change: container already covers this range
                continue;
            }
        }

        stack.push(entry);
    }

    // Drain remaining stack
    while let Some(entry) = stack.pop() {
        output.push(entry);
    }

    output
}

fn contains_v4(outer: &Ipv4Net, inner: &Ipv4Net) -> bool {
    if outer.prefix_len() > inner.prefix_len() {
        return false;
    }
    let outer_bits = u32::from(outer.network());
    let inner_bits = u32::from(inner.network());
    let mask = if outer.prefix_len() == 0 {
        0
    } else {
        !0u32 << (32 - outer.prefix_len())
    };
    (outer_bits & mask) == (inner_bits & mask)
}

/// Stack-based sibling merging with cascading.
fn sibling_merge_v4(entries: &mut Vec<SourceMapPrefix<Ipv4Net>>) {
    if entries.len() <= 1 {
        return;
    }

    // Sort by (prefix_length DESC, network_address ASC)
    entries.sort_unstable_by(|a, b| {
        b.prefix
            .prefix_len()
            .cmp(&a.prefix.prefix_len())
            .then_with(|| {
                u32::from(a.prefix.network()).cmp(&u32::from(b.prefix.network()))
            })
    });

    let mut stack: Vec<SourceMapPrefix<Ipv4Net>> = Vec::new();

    for entry in entries.drain(..) {
        stack.push(entry);

        // Check for sibling merges (cascading)
        while stack.len() >= 2 {
            let len = stack.len();
            if are_siblings_v4(&stack[len - 2].prefix, &stack[len - 1].prefix) {
                let right = stack.pop().unwrap();
                let left = stack.pop().unwrap();
                let parent_len = left.prefix.prefix_len() - 1;
                let parent_net = trunc_v4(left.prefix.network(), parent_len);
                let merged_coverage = left.coverage.saturating_add(right.coverage);
                let mut merged_indices = left.source_indices;
                merged_indices.extend(right.source_indices);
                stack.push(SourceMapPrefix {
                    prefix: Ipv4Net::new(parent_net, parent_len).unwrap(),
                    source_indices: merged_indices,
                    coverage: merged_coverage,
                    preferred_overlap_in_coverage: 0,
                });
            } else {
                break;
            }
        }
    }

    *entries = stack;
}

fn are_siblings_v4(a: &Ipv4Net, b: &Ipv4Net) -> bool {
    if a.prefix_len() != b.prefix_len() || a.prefix_len() == 0 {
        return false;
    }
    let len = a.prefix_len();
    let a_bits = u32::from(a.network());
    let b_bits = u32::from(b.network());
    // Siblings differ only in the bit at position (len - 1)
    let diff = a_bits ^ b_bits;
    diff == (1u32 << (32 - len))
}

// --- IPv6 helpers ---

fn trunc_v6(addr: Ipv6Addr, prefix_len: u8) -> Ipv6Addr {
    let bits = u128::from(addr);
    let mask = if prefix_len == 0 {
        0
    } else {
        !0u128 << (128 - prefix_len)
    };
    Ipv6Addr::from(bits & mask)
}

fn radix_sort_v6(entries: &mut Vec<SourceMapPrefix<Ipv6Net>>) {
    let len = entries.len();
    if len <= 1 {
        return;
    }

    // Key: 17 bytes = [prefix_len, addr[15], addr[14], ..., addr[0]]
    let key_bytes: Vec<[u8; 17]> = entries
        .iter()
        .map(|e| {
            let octets = e.prefix.network().octets();
            let mut key = [0u8; 17];
            key[0] = e.prefix.prefix_len();
            for i in 0..16 {
                key[1 + i] = octets[15 - i]; // LSD order
            }
            key
        })
        .collect();

    let mut indices: Vec<usize> = (0..len).collect();
    let mut indices_buf: Vec<usize> = vec![0; len];

    for pass in 0..17 {
        let mut counts = [0u32; 256];
        for &i in &indices {
            counts[key_bytes[i][pass] as usize] += 1;
        }
        let mut offsets = [0u32; 256];
        for i in 1..256 {
            offsets[i] = offsets[i - 1] + counts[i - 1];
        }
        for &i in &indices {
            let byte = key_bytes[i][pass] as usize;
            indices_buf[offsets[byte] as usize] = i;
            offsets[byte] += 1;
        }
        std::mem::swap(&mut indices, &mut indices_buf);
    }

    let mut sorted = Vec::with_capacity(len);
    for &i in &indices {
        sorted.push(std::mem::replace(
            &mut entries[i],
            SourceMapPrefix {
                prefix: Ipv6Net::new(Ipv6Addr::UNSPECIFIED, 0).unwrap(),
                source_indices: Vec::new(),
                coverage: 0,
                preferred_overlap_in_coverage: 0,
            },
        ));
    }
    *entries = sorted;
}

fn redundancy_eliminate_v6(
    entries: Vec<SourceMapPrefix<Ipv6Net>>,
) -> Vec<SourceMapPrefix<Ipv6Net>> {
    let mut stack: Vec<SourceMapPrefix<Ipv6Net>> = Vec::new();
    let mut output: Vec<SourceMapPrefix<Ipv6Net>> = Vec::new();

    for entry in entries {
        while let Some(top) = stack.last() {
            if contains_v6(&top.prefix, &entry.prefix) {
                break;
            }
            output.push(stack.pop().unwrap());
        }

        if let Some(top) = stack.last_mut() {
            if contains_v6(&top.prefix, &entry.prefix) {
                top.source_indices.extend(&entry.source_indices);
                // Coverage doesn't change: container already covers this range
                continue;
            }
        }

        stack.push(entry);
    }

    while let Some(entry) = stack.pop() {
        output.push(entry);
    }

    output
}

fn contains_v6(outer: &Ipv6Net, inner: &Ipv6Net) -> bool {
    if outer.prefix_len() > inner.prefix_len() {
        return false;
    }
    let outer_bits = u128::from(outer.network());
    let inner_bits = u128::from(inner.network());
    let mask = if outer.prefix_len() == 0 {
        0
    } else {
        !0u128 << (128 - outer.prefix_len())
    };
    (outer_bits & mask) == (inner_bits & mask)
}

fn sibling_merge_v6(entries: &mut Vec<SourceMapPrefix<Ipv6Net>>) {
    if entries.len() <= 1 {
        return;
    }

    entries.sort_unstable_by(|a, b| {
        b.prefix
            .prefix_len()
            .cmp(&a.prefix.prefix_len())
            .then_with(|| {
                u128::from(a.prefix.network()).cmp(&u128::from(b.prefix.network()))
            })
    });

    let mut stack: Vec<SourceMapPrefix<Ipv6Net>> = Vec::new();

    for entry in entries.drain(..) {
        stack.push(entry);

        while stack.len() >= 2 {
            let len = stack.len();
            if are_siblings_v6(&stack[len - 2].prefix, &stack[len - 1].prefix) {
                let right = stack.pop().unwrap();
                let left = stack.pop().unwrap();
                let parent_len = left.prefix.prefix_len() - 1;
                let parent_net = trunc_v6(left.prefix.network(), parent_len);
                let merged_coverage = left.coverage.saturating_add(right.coverage);
                let mut merged_indices = left.source_indices;
                merged_indices.extend(right.source_indices);
                stack.push(SourceMapPrefix {
                    prefix: Ipv6Net::new(parent_net, parent_len).unwrap(),
                    source_indices: merged_indices,
                    coverage: merged_coverage,
                    preferred_overlap_in_coverage: 0,
                });
            } else {
                break;
            }
        }
    }

    *entries = stack;
}

fn are_siblings_v6(a: &Ipv6Net, b: &Ipv6Net) -> bool {
    if a.prefix_len() != b.prefix_len() || a.prefix_len() == 0 {
        return false;
    }
    let len = a.prefix_len();
    let a_bits = u128::from(a.network());
    let b_bits = u128::from(b.network());
    let diff = a_bits ^ b_bits;
    diff == (1u128 << (128 - len))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redundancy_removal() {
        // /8 subsumes /16
        let input = vec![
            (0, "10.0.0.0/8".parse().unwrap()),
            (1, "10.1.0.0/16".parse().unwrap()),
        ];
        let result = lossless_aggregate_v4(input, 32);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].prefix, "10.0.0.0/8".parse::<Ipv4Net>().unwrap());
        assert!(result[0].source_indices.contains(&0));
        assert!(result[0].source_indices.contains(&1));
    }

    #[test]
    fn sibling_merge() {
        // 10.0.0.0/25 + 10.0.0.128/25 → 10.0.0.0/24
        let input = vec![
            (0, "10.0.0.0/25".parse().unwrap()),
            (1, "10.0.0.128/25".parse().unwrap()),
        ];
        let result = lossless_aggregate_v4(input, 32);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].prefix, "10.0.0.0/24".parse::<Ipv4Net>().unwrap());
        assert_eq!(result[0].source_indices.len(), 2);
    }

    #[test]
    fn cascading_merge() {
        // Four /26s that merge to /24
        let input = vec![
            (0, "10.0.0.0/26".parse().unwrap()),
            (1, "10.0.0.64/26".parse().unwrap()),
            (2, "10.0.0.128/26".parse().unwrap()),
            (3, "10.0.0.192/26".parse().unwrap()),
        ];
        let result = lossless_aggregate_v4(input, 32);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].prefix, "10.0.0.0/24".parse::<Ipv4Net>().unwrap());
        assert_eq!(result[0].source_indices.len(), 4);
    }

    #[test]
    fn max_prefix_len_truncation() {
        let input = vec![
            (0, "10.0.0.1/32".parse().unwrap()),
            (1, "10.0.0.2/32".parse().unwrap()),
        ];
        let result = lossless_aggregate_v4(input, 24);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].prefix, "10.0.0.0/24".parse::<Ipv4Net>().unwrap());
    }

    #[test]
    fn source_map_through_merges() {
        let input = vec![
            (0, "10.0.0.0/25".parse().unwrap()),
            (1, "10.0.0.128/25".parse().unwrap()),
            (2, "10.0.0.64/26".parse().unwrap()), // subsumed by 10.0.0.0/25
        ];
        let result = lossless_aggregate_v4(input, 32);
        assert_eq!(result.len(), 1);
        // All three original indices should be tracked
        let mut indices = result[0].source_indices.clone();
        indices.sort();
        assert_eq!(indices, vec![0, 1, 2]);
    }

    #[test]
    fn ipv6_sibling_merge() {
        let input = vec![
            (0, "2001:db8::/33".parse().unwrap()),
            (1, "2001:db8:8000::/33".parse().unwrap()),
        ];
        let result = lossless_aggregate_v6(input, 128);
        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0].prefix,
            "2001:db8::/32".parse::<Ipv6Net>().unwrap()
        );
    }

    #[test]
    fn empty_input() {
        let result = lossless_aggregate_v4(vec![], 32);
        assert!(result.is_empty());
    }

    #[test]
    fn single_entry() {
        let input = vec![(0, "10.0.0.0/8".parse().unwrap())];
        let result = lossless_aggregate_v4(input, 32);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].prefix, "10.0.0.0/8".parse::<Ipv4Net>().unwrap());
    }

    #[test]
    fn no_merge_non_siblings() {
        let input = vec![
            (0, "10.0.0.0/24".parse().unwrap()),
            (1, "10.0.2.0/24".parse().unwrap()),
        ];
        let result = lossless_aggregate_v4(input, 32);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn differential_vs_ipnet_aggregate() {
        let inputs: Vec<Ipv4Net> = vec![
            "10.0.0.0/24",
            "10.0.1.0/24",
            "10.0.2.0/24",
            "10.0.3.0/24",
            "192.168.0.0/24",
            "192.168.1.0/24",
        ]
        .into_iter()
        .map(|s| s.parse().unwrap())
        .collect();

        // Our implementation
        let indexed: Vec<(usize, Ipv4Net)> =
            inputs.iter().copied().enumerate().collect();
        let our_result = lossless_aggregate_v4(indexed, 32);
        let mut our_prefixes: Vec<Ipv4Net> =
            our_result.iter().map(|e| e.prefix).collect();
        our_prefixes.sort_by_key(|p| (u32::from(p.network()), p.prefix_len()));

        // ipnet::Ipv4Net::aggregate
        let ipnet_result = Ipv4Net::aggregate(&inputs.clone());
        let mut ipnet_prefixes = ipnet_result;
        ipnet_prefixes.sort_by_key(|p| (u32::from(p.network()), p.prefix_len()));

        assert_eq!(our_prefixes, ipnet_prefixes);
    }
}
