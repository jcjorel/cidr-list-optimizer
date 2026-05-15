mod common;

use std::net::Ipv4Addr;

use cidr_optimizer::{optimize, validate_coverage, OptimizerConfig};
use ipnet::IpNet;

use common::{generate_contiguous_v4, minimal_prefix_decomposition_count, time_it};

#[test]
fn test_10k_sequential_v4() {
    let base = Ipv4Addr::new(10, 0, 0, 0);
    let input = generate_contiguous_v4(base, 10_000);
    let expected_count = minimal_prefix_decomposition_count(0x0A000000, 10_000);

    let result = time_it("10k_sequential_v4", || {
        optimize(&input, &OptimizerConfig::default()).unwrap()
    });

    assert!(validate_coverage(&input, &result.entries));
    assert_eq!(result.entries.len(), expected_count);
    eprintln!("  output entries: {}", result.entries.len());
}

#[test]
fn test_65536_sequential_v4() {
    let base = Ipv4Addr::new(10, 0, 0, 0);
    let input = generate_contiguous_v4(base, 65_536);

    let result = time_it("65536_sequential_v4", || {
        optimize(&input, &OptimizerConfig::default()).unwrap()
    });

    assert!(validate_coverage(&input, &result.entries));
    assert_eq!(result.entries.len(), 1);
    let expected: IpNet = "10.0.0.0/16".parse().unwrap();
    assert_eq!(result.entries[0].prefix, expected);
    eprintln!("  output entries: {}", result.entries.len());
}

#[test]
#[cfg_attr(not(feature = "stress"), ignore)]
fn test_100k_sequential_v4() {
    let base = Ipv4Addr::new(10, 0, 0, 0);
    let input = generate_contiguous_v4(base, 100_000);
    let expected_count = minimal_prefix_decomposition_count(0x0A000000, 100_000);

    let result = time_it("100k_sequential_v4", || {
        optimize(&input, &OptimizerConfig::default()).unwrap()
    });

    assert!(validate_coverage(&input, &result.entries));
    assert_eq!(result.entries.len(), expected_count);
    eprintln!("  output entries: {}", result.entries.len());
}

#[test]
#[cfg_attr(not(feature = "stress"), ignore)]
fn test_1m_sequential_v4() {
    let base = Ipv4Addr::new(10, 0, 0, 0);
    let input = generate_contiguous_v4(base, 1_048_576);

    let result = time_it("1m_sequential_v4", || {
        optimize(&input, &OptimizerConfig::default()).unwrap()
    });

    assert!(validate_coverage(&input, &result.entries));
    assert_eq!(result.entries.len(), 1);
    let expected: IpNet = "10.0.0.0/12".parse().unwrap();
    assert_eq!(result.entries[0].prefix, expected);
    eprintln!("  output entries: {}", result.entries.len());
}
