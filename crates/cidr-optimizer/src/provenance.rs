//! Binary-search provenance mapping from output prefixes back to original input indices.

use ipnet::{Ipv4Net, Ipv6Net};

/// For each output prefix, find all input prefixes it contains via binary search.
pub fn compute_provenance_v4(
    output: &[Ipv4Net],
    sorted_input: &[(usize, Ipv4Net)],
) -> Vec<Vec<usize>> {
    output
        .iter()
        .map(|out| {
            let out_start = u32::from(out.network());
            let out_end = u32::from(out.broadcast());
            // Binary search for first input whose network >= out.network()
            let start = sorted_input.partition_point(|(_, p)| u32::from(p.network()) < out_start);
            // Binary search for first input whose network > out.broadcast()
            let end = sorted_input.partition_point(|(_, p)| u32::from(p.network()) <= out_end);
            sorted_input[start..end]
                .iter()
                .filter(|(_, p)| contains_v4(out, p))
                .map(|(idx, _)| *idx)
                .collect()
        })
        .collect()
}

/// For each output prefix, find all input prefixes it contains via binary search.
pub fn compute_provenance_v6(
    output: &[Ipv6Net],
    sorted_input: &[(usize, Ipv6Net)],
) -> Vec<Vec<usize>> {
    output
        .iter()
        .map(|out| {
            let out_start = u128::from(out.network());
            let out_broadcast = broadcast_v6(out);
            let start = sorted_input.partition_point(|(_, p)| u128::from(p.network()) < out_start);
            let end = sorted_input.partition_point(|(_, p)| u128::from(p.network()) <= out_broadcast);
            sorted_input[start..end]
                .iter()
                .filter(|(_, p)| contains_v6(out, p))
                .map(|(idx, _)| *idx)
                .collect()
        })
        .collect()
}

fn contains_v4(outer: &Ipv4Net, inner: &Ipv4Net) -> bool {
    if outer.prefix_len() > inner.prefix_len() {
        return false;
    }
    let mask = if outer.prefix_len() == 0 { 0 } else { !0u32 << (32 - outer.prefix_len()) };
    (u32::from(outer.network()) & mask) == (u32::from(inner.network()) & mask)
}

fn contains_v6(outer: &Ipv6Net, inner: &Ipv6Net) -> bool {
    if outer.prefix_len() > inner.prefix_len() {
        return false;
    }
    let mask = if outer.prefix_len() == 0 { 0 } else { !0u128 << (128 - outer.prefix_len()) };
    (u128::from(outer.network()) & mask) == (u128::from(inner.network()) & mask)
}

fn broadcast_v6(net: &Ipv6Net) -> u128 {
    let bits = u128::from(net.network());
    if net.prefix_len() == 0 {
        u128::MAX
    } else if net.prefix_len() == 128 {
        bits
    } else {
        bits | ((1u128 << (128 - net.prefix_len())) - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provenance_v4_basic() {
        let output: Vec<Ipv4Net> = vec!["10.0.0.0/23".parse().unwrap()];
        let input: Vec<(usize, Ipv4Net)> = vec![
            (0, "10.0.0.0/24".parse().unwrap()),
            (1, "10.0.1.0/24".parse().unwrap()),
        ];
        let result = compute_provenance_v4(&output, &input);
        assert_eq!(result, vec![vec![0, 1]]);
    }

    #[test]
    fn provenance_v4_partial() {
        let output: Vec<Ipv4Net> = vec![
            "10.0.0.0/24".parse().unwrap(),
            "10.0.1.0/24".parse().unwrap(),
        ];
        let input: Vec<(usize, Ipv4Net)> = vec![
            (0, "10.0.0.0/24".parse().unwrap()),
            (1, "10.0.1.0/24".parse().unwrap()),
            (2, "192.168.0.0/24".parse().unwrap()),
        ];
        let result = compute_provenance_v4(&output, &input);
        assert_eq!(result, vec![vec![0], vec![1]]);
    }

    #[test]
    fn provenance_v6_basic() {
        let output: Vec<Ipv6Net> = vec!["2001:db8::/47".parse().unwrap()];
        let input: Vec<(usize, Ipv6Net)> = vec![
            (0, "2001:db8::/48".parse().unwrap()),
            (1, "2001:db8:1::/48".parse().unwrap()),
        ];
        let result = compute_provenance_v6(&output, &input);
        assert_eq!(result, vec![vec![0, 1]]);
    }
}
