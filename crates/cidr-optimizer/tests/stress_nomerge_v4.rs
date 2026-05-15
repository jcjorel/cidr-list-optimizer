mod common;

use std::net::Ipv4Addr;

use cidr_optimizer::{optimize, validate_coverage, OptimizerConfig};
use ipnet::IpNet;

use common::time_it;

/// Generate N non-adjacent /32s at even addresses (stride=2) that cannot merge.
fn generate_nomerge_v4(count: u32) -> Vec<IpNet> {
    let base = u32::from(Ipv4Addr::new(10, 0, 0, 0));
    (0..count)
        .map(|i| {
            let addr = Ipv4Addr::from(base + i * 2);
            IpNet::V4(ipnet::Ipv4Net::new(addr, 32).unwrap())
        })
        .collect()
}

#[test]
fn test_10k_nomerge_v4() {
    let input = generate_nomerge_v4(10_000);

    let result = time_it("10k_nomerge_v4", || {
        optimize(&input, &OptimizerConfig::default()).unwrap()
    });

    assert!(validate_coverage(&input, &result.entries));
    assert_eq!(result.entries.len(), 10_000);
    eprintln!("  output entries: {}", result.entries.len());
}

#[test]
#[cfg_attr(not(feature = "stress"), ignore)]
fn test_100k_nomerge_v4() {
    let input = generate_nomerge_v4(100_000);

    let result = time_it("100k_nomerge_v4", || {
        optimize(&input, &OptimizerConfig::default()).unwrap()
    });

    assert!(validate_coverage(&input, &result.entries));
    assert_eq!(result.entries.len(), 100_000);
    eprintln!("  output entries: {}", result.entries.len());
}

#[test]
#[cfg_attr(not(feature = "stress"), ignore)]
fn test_1m_nomerge_v4() {
    let input = generate_nomerge_v4(1_000_000);

    let result = time_it("1m_nomerge_v4", || {
        optimize(&input, &OptimizerConfig::default()).unwrap()
    });

    assert!(validate_coverage(&input, &result.entries));
    assert_eq!(result.entries.len(), 1_000_000);
    eprintln!("  output entries: {}", result.entries.len());
}
