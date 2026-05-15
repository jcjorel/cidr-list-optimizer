mod common;

use std::net::Ipv4Addr;

use cidr_optimizer::{optimize, validate_coverage, OptimizerConfig};
use ipnet::IpNet;

use common::{minimal_prefix_decomposition_count, time_it};

/// Generate N contiguous /24 subnets fully populated with 256 /32s each.
/// Base starts at 10.0.0.0.
fn generate_full_subnets(num_subnets: u32) -> Vec<IpNet> {
    let base: u32 = 0x0A000000; // 10.0.0.0
    let total = num_subnets * 256;
    (0..total)
        .map(|i| {
            let addr = Ipv4Addr::from(base + i);
            IpNet::V4(ipnet::Ipv4Net::new(addr, 32).unwrap())
        })
        .collect()
}

#[test]
fn test_10k_full_subnets_v4() {
    let num_subnets = 40u32; // 40 * 256 = 10240 inputs
    let input = generate_full_subnets(num_subnets);
    let expected_count = minimal_prefix_decomposition_count(0x0A000000, num_subnets * 256);

    let result = time_it("10k_full_subnets_v4", || {
        optimize(&input, &OptimizerConfig::default()).unwrap()
    });

    assert!(validate_coverage(&input, &result.entries));
    assert_eq!(result.entries.len(), expected_count);
    eprintln!("  output entries: {}", result.entries.len());
}

#[test]
#[cfg_attr(not(feature = "stress"), ignore)]
fn test_100k_full_subnets_v4() {
    let num_subnets = 400u32; // 400 * 256 = 102400 inputs
    let input = generate_full_subnets(num_subnets);
    let expected_count = minimal_prefix_decomposition_count(0x0A000000, num_subnets * 256);

    let result = time_it("100k_full_subnets_v4", || {
        optimize(&input, &OptimizerConfig::default()).unwrap()
    });

    assert!(validate_coverage(&input, &result.entries));
    assert_eq!(result.entries.len(), expected_count);
    eprintln!("  output entries: {}", result.entries.len());
}

#[test]
#[cfg_attr(not(feature = "stress"), ignore)]
fn test_1m_full_subnets_v4() {
    let num_subnets = 4096u32; // 4096 * 256 = 1048576 inputs
    let input = generate_full_subnets(num_subnets);

    let result = time_it("1m_full_subnets_v4", || {
        optimize(&input, &OptimizerConfig::default()).unwrap()
    });

    assert!(validate_coverage(&input, &result.entries));
    assert_eq!(result.entries.len(), 1);
    let expected: IpNet = "10.0.0.0/12".parse().unwrap();
    assert_eq!(result.entries[0].prefix, expected);
    eprintln!("  output entries: {}", result.entries.len());
}
