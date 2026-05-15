mod common;

use std::net::{Ipv4Addr, Ipv6Addr};

use cidr_optimizer::{optimize, validate_coverage, OptimizerConfig};
use ipnet::{IpNet, Ipv4Net, Ipv6Net};

use common::{generate_contiguous_v4, generate_contiguous_v6, time_it};

/// Convert our output entries to a sorted Vec<IpNet> for comparison.
fn sorted_output(result: &cidr_optimizer::OptimizationResult) -> Vec<IpNet> {
    let mut nets: Vec<IpNet> = result.entries.iter().map(|e| e.prefix).collect();
    nets.sort_by(|a, b| match (a, b) {
        (IpNet::V4(a4), IpNet::V4(b4)) => {
            u32::from(a4.network()).cmp(&u32::from(b4.network()))
                .then(a4.prefix_len().cmp(&b4.prefix_len()))
        }
        (IpNet::V6(a6), IpNet::V6(b6)) => {
            u128::from(a6.network()).cmp(&u128::from(b6.network()))
                .then(a6.prefix_len().cmp(&b6.prefix_len()))
        }
        (IpNet::V4(_), IpNet::V6(_)) => std::cmp::Ordering::Less,
        (IpNet::V6(_), IpNet::V4(_)) => std::cmp::Ordering::Greater,
    });
    nets
}

/// Run ipnet aggregate on a set of IpNets, return sorted result.
fn ipnet_aggregate_sorted(input: &[IpNet]) -> Vec<IpNet> {
    let mut v4s: Vec<Ipv4Net> = Vec::new();
    let mut v6s: Vec<Ipv6Net> = Vec::new();
    for net in input {
        match net {
            IpNet::V4(v4) => v4s.push(*v4),
            IpNet::V6(v6) => v6s.push(*v6),
        }
    }
    let agg_v4 = Ipv4Net::aggregate(&v4s);
    let agg_v6 = Ipv6Net::aggregate(&v6s);
    let mut result: Vec<IpNet> = agg_v4.into_iter().map(IpNet::V4)
        .chain(agg_v6.into_iter().map(IpNet::V6))
        .collect();
    result.sort_by(|a, b| match (a, b) {
        (IpNet::V4(a4), IpNet::V4(b4)) => {
            u32::from(a4.network()).cmp(&u32::from(b4.network()))
                .then(a4.prefix_len().cmp(&b4.prefix_len()))
        }
        (IpNet::V6(a6), IpNet::V6(b6)) => {
            u128::from(a6.network()).cmp(&u128::from(b6.network()))
                .then(a6.prefix_len().cmp(&b6.prefix_len()))
        }
        (IpNet::V4(_), IpNet::V6(_)) => std::cmp::Ordering::Less,
        (IpNet::V6(_), IpNet::V4(_)) => std::cmp::Ordering::Greater,
    });
    result
}

fn run_differential(input: &[IpNet], label: &str) {
    let our_result = time_it(&format!("{} (ours)", label), || {
        optimize(input, &OptimizerConfig::default()).unwrap()
    });
    let ipnet_result = time_it(&format!("{} (ipnet)", label), || {
        ipnet_aggregate_sorted(input)
    });

    assert!(validate_coverage(input, &our_result.entries));
    assert_eq!(sorted_output(&our_result), ipnet_result,
        "differential mismatch for {}", label);
}

#[test]
fn test_10k_differential_contiguous_v4() {
    let input = generate_contiguous_v4(Ipv4Addr::new(10, 0, 0, 0), 10_000);
    run_differential(&input, "10k_contiguous_v4");
}

#[test]
fn test_10k_differential_contiguous_v24() {
    let input: Vec<IpNet> = (0u32..10_000)
        .map(|i| {
            let addr = Ipv4Addr::from(0x0A000000 + i * 256);
            IpNet::V4(Ipv4Net::new(addr, 24).unwrap())
        })
        .collect();
    run_differential(&input, "10k_contiguous_v24");
}

#[test]
fn test_10k_differential_mixed() {
    let mut input: Vec<IpNet> = Vec::with_capacity(10_000);
    // 3333 /16s within 10.0.0.0/8
    for i in 0u32..3333 {
        let addr = Ipv4Addr::from(0x0A000000 + (i % 256) * 0x10000);
        input.push(IpNet::V4(Ipv4Net::new(addr, 16).unwrap()));
    }
    // 3333 /24s within 10.0.0.0/8
    for i in 0u32..3333 {
        let addr = Ipv4Addr::from(0x0A000000 + (i % 256) * 256);
        input.push(IpNet::V4(Ipv4Net::new(addr, 24).unwrap()));
    }
    // 3334 /32s within 10.0.0.0/8
    for i in 0u32..3334 {
        let addr = Ipv4Addr::from(0x0A000000 + i);
        input.push(IpNet::V4(Ipv4Net::new(addr, 32).unwrap()));
    }
    run_differential(&input, "10k_mixed");
}

#[test]
fn test_10k_differential_v6() {
    let input = generate_contiguous_v6(Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 0), 10_000);
    run_differential(&input, "10k_v6");
}

#[test]
#[cfg_attr(not(feature = "stress"), ignore)]
fn test_100k_differential_v4() {
    let input = generate_contiguous_v4(Ipv4Addr::new(10, 0, 0, 0), 100_000);
    run_differential(&input, "100k_v4");
}

#[test]
#[cfg_attr(not(feature = "stress"), ignore)]
fn test_100k_differential_v6() {
    let input = generate_contiguous_v6(Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 0), 100_000);
    run_differential(&input, "100k_v6");
}

#[test]
#[cfg_attr(not(feature = "stress"), ignore)]
fn test_1m_differential_v4() {
    let input = generate_contiguous_v4(Ipv4Addr::new(10, 0, 0, 0), 1_000_000);
    run_differential(&input, "1m_v4");
}

#[test]
#[cfg_attr(not(feature = "stress"), ignore)]
fn test_1m_differential_v6() {
    let input = generate_contiguous_v6(Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 0), 1_000_000);
    run_differential(&input, "1m_v6");
}
