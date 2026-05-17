//! Binary-search source-map mapping from output prefixes back to original input indices.

use ipnet::{Ipv4Net, Ipv6Net};

/// For each output prefix, find all input prefixes it contains via binary search.
pub fn compute_source_map_v4(
    output: &[Ipv4Net],
    sorted_input: &[(usize, Ipv4Net)],
) -> Vec<Vec<usize>> {
    // Map each output prefix to the list of original input indices it covers
    output
        .iter()
        .map(|out| {
            let out_start = u32::from(out.network());
            let out_end = u32::from(out.broadcast());
            // Narrow search window: skip inputs starting before this output prefix
            let start = sorted_input.partition_point(|(_, p)| u32::from(p.network()) < out_start);
            // Upper bound: first input that starts beyond this output prefix's range
            let end = sorted_input.partition_point(|(_, p)| u32::from(p.network()) <= out_end);
            // Binary search only bounds by start address; filter out inputs that extend past the output prefix
            sorted_input[start..end]
                .iter()
                .filter(|(_, p)| contains_v4(out, p))
                .map(|(idx, _)| *idx)
                .collect()
        })
        .collect()
}

/// For each output prefix, find all input prefixes it contains via binary search.
pub fn compute_source_map_v6(
    output: &[Ipv6Net],
    sorted_input: &[(usize, Ipv6Net)],
) -> Vec<Vec<usize>> {
    // Map each output prefix to the list of original input indices it covers
    output
        .iter()
        .map(|out| {
            let out_start = u128::from(out.network());
            let out_broadcast = broadcast_v6(out);
            // Narrow search window: skip inputs starting before this output prefix
            let start = sorted_input.partition_point(|(_, p)| u128::from(p.network()) < out_start);
            // Upper bound: first input that starts beyond this output prefix's range
            let end = sorted_input.partition_point(|(_, p)| u128::from(p.network()) <= out_broadcast);
            // Binary search only bounds by start address; filter out inputs that extend past the output prefix
            sorted_input[start..end]
                .iter()
                .filter(|(_, p)| contains_v6(out, p))
                .map(|(idx, _)| *idx)
                .collect()
        })
        .collect()
}

/// Returns true if `inner` is fully contained within `outer` (prefix containment check).
fn contains_v4(outer: &Ipv4Net, inner: &Ipv4Net) -> bool {
    // A more-specific outer cannot contain a less-specific inner
    if outer.prefix_len() > inner.prefix_len() {
        return false;
    }
    // Mask isolates the network portion; /0 trivially contains everything
    let mask = if outer.prefix_len() == 0 { 0 } else { !0u32 << (32 - outer.prefix_len()) };
    (u32::from(outer.network()) & mask) == (u32::from(inner.network()) & mask)
}

/// Returns true if `inner` IPv6 prefix is fully contained within `outer`.
fn contains_v6(outer: &Ipv6Net, inner: &Ipv6Net) -> bool {
    // A more-specific outer cannot contain a less-specific inner
    if outer.prefix_len() > inner.prefix_len() {
        return false;
    }
    // Mask isolates the network portion; /0 trivially contains everything
    let mask = if outer.prefix_len() == 0 { 0 } else { !0u128 << (128 - outer.prefix_len()) };
    (u128::from(outer.network()) & mask) == (u128::from(inner.network()) & mask)
}

/// Computes the broadcast (last) address of an IPv6 network as u128.
fn broadcast_v6(net: &Ipv6Net) -> u128 {
    let bits = u128::from(net.network());
    // Edge case: /0 means all bits are host bits; the shift in the general formula would overflow (128-bit shift), so return MAX directly
    if net.prefix_len() == 0 {
        u128::MAX
    // /128 is a host route — broadcast equals network
    } else if net.prefix_len() == 128 {
        bits
    } else {
        // General case (/1–/127): fill host portion with ones to get the last address
        bits | ((1u128 << (128 - net.prefix_len())) - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_map_v4_basic() {
        let output: Vec<Ipv4Net> = vec!["10.0.0.0/23".parse().unwrap()];
        let input: Vec<(usize, Ipv4Net)> = vec![
            (0, "10.0.0.0/24".parse().unwrap()),
            (1, "10.0.1.0/24".parse().unwrap()),
        ];
        let result = compute_source_map_v4(&output, &input);
        assert_eq!(result, vec![vec![0, 1]]);
    }

    #[test]
    fn source_map_v4_partial() {
        let output: Vec<Ipv4Net> = vec![
            "10.0.0.0/24".parse().unwrap(),
            "10.0.1.0/24".parse().unwrap(),
        ];
        let input: Vec<(usize, Ipv4Net)> = vec![
            (0, "10.0.0.0/24".parse().unwrap()),
            (1, "10.0.1.0/24".parse().unwrap()),
            (2, "192.168.0.0/24".parse().unwrap()),
        ];
        let result = compute_source_map_v4(&output, &input);
        assert_eq!(result, vec![vec![0], vec![1]]);
    }

    #[test]
    fn source_map_v6_basic() {
        let output: Vec<Ipv6Net> = vec!["2001:db8::/47".parse().unwrap()];
        let input: Vec<(usize, Ipv6Net)> = vec![
            (0, "2001:db8::/48".parse().unwrap()),
            (1, "2001:db8:1::/48".parse().unwrap()),
        ];
        let result = compute_source_map_v6(&output, &input);
        assert_eq!(result, vec![vec![0, 1]]);
    }
}
