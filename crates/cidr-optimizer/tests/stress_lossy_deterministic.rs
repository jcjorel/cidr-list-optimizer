mod common;

use std::net::Ipv4Addr;

use cidr_optimizer::{optimize, validate_coverage, OptimizerConfig};
use ipnet::IpNet;

use common::time_it;

/// Generate N evenly-spaced /32s across the IPv4 address space.
fn generate_spaced_v4(count: u32) -> Vec<IpNet> {
    let stride = (u32::MAX as u64 + 1) / count as u64;
    (0..count)
        .map(|i| {
            let addr = Ipv4Addr::from((i as u64 * stride) as u32);
            IpNet::V4(ipnet::Ipv4Net::new(addr, 32).unwrap())
        })
        .collect()
}

#[test]
fn test_10k_lossy_target_100() {
    let input = generate_spaced_v4(10_000);
    let config = OptimizerConfig {
        ipv4_target: Some(100),
        ..Default::default()
    };

    let result = time_it("10k_lossy_target_100", || {
        optimize(&input, &config).unwrap()
    });

    assert!(validate_coverage(&input, &result.entries));
    assert!(result.entries.len() <= 100);
    eprintln!("  output entries: {}", result.entries.len());
}

#[test]
fn test_10k_lossy_target_10() {
    let input = generate_spaced_v4(10_000);
    let config = OptimizerConfig {
        ipv4_target: Some(10),
        ..Default::default()
    };

    let result = time_it("10k_lossy_target_10", || {
        optimize(&input, &config).unwrap()
    });

    assert!(validate_coverage(&input, &result.entries));
    assert!(result.entries.len() <= 10);
    eprintln!("  output entries: {}", result.entries.len());
}

#[test]
#[cfg_attr(not(feature = "stress"), ignore)]
fn test_100k_lossy_target_100() {
    let input = generate_spaced_v4(100_000);
    let config = OptimizerConfig {
        ipv4_target: Some(100),
        ..Default::default()
    };

    let result = time_it("100k_lossy_target_100", || {
        optimize(&input, &config).unwrap()
    });

    assert!(validate_coverage(&input, &result.entries));
    assert!(result.entries.len() <= 100);
    eprintln!("  output entries: {}", result.entries.len());
}

#[test]
#[cfg_attr(not(feature = "stress"), ignore)]
fn test_100k_lossy_target_10() {
    let input = generate_spaced_v4(100_000);
    let config = OptimizerConfig {
        ipv4_target: Some(10),
        ..Default::default()
    };

    let result = time_it("100k_lossy_target_10", || {
        optimize(&input, &config).unwrap()
    });

    assert!(validate_coverage(&input, &result.entries));
    assert!(result.entries.len() <= 10);
    eprintln!("  output entries: {}", result.entries.len());
}

#[test]
#[cfg_attr(not(feature = "stress"), ignore)]
fn test_1m_lossy_target_100() {
    let input = generate_spaced_v4(1_000_000);
    let config = OptimizerConfig {
        ipv4_target: Some(100),
        ..Default::default()
    };

    let result = time_it("1m_lossy_target_100", || {
        optimize(&input, &config).unwrap()
    });

    assert!(validate_coverage(&input, &result.entries));
    assert!(result.entries.len() <= 100);
    eprintln!("  output entries: {}", result.entries.len());
}

#[test]
#[cfg_attr(not(feature = "stress"), ignore)]
fn test_1m_lossy_target_10() {
    let input = generate_spaced_v4(1_000_000);
    let config = OptimizerConfig {
        ipv4_target: Some(10),
        ..Default::default()
    };

    let result = time_it("1m_lossy_target_10", || {
        optimize(&input, &config).unwrap()
    });

    assert!(validate_coverage(&input, &result.entries));
    assert!(result.entries.len() <= 10);
    eprintln!("  output entries: {}", result.entries.len());
}
