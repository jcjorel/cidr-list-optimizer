mod common;

use std::collections::HashSet;
use std::net::{Ipv4Addr, Ipv6Addr};

use cidr_optimizer::{optimize, validate_coverage, OptimizerConfig, TargetSpec};
use ipnet::IpNet;

use common::{generate_contiguous_v4, generate_contiguous_v6, time_it};

// --- 8a: Mixed IPv4+IPv6 with source-map ---

fn run_mixed_source_map(n: usize, label: &str) {
    let half = n / 2;
    let v4 = generate_contiguous_v4(Ipv4Addr::new(10, 0, 0, 0), half as u32);
    let v6 = generate_contiguous_v6(Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 0), half as u64);
    let input: Vec<IpNet> = v4.into_iter().chain(v6.into_iter()).collect();

    let config = OptimizerConfig {
        source_map: true,
        ..Default::default()
    };

    let result = time_it(label, || optimize(&input, &config).unwrap());
    assert!(validate_coverage(&input, &result.entries));

    for entry in &result.entries {
        let indices = entry.source_indices.as_ref().expect("source_map must be set");
        match entry.prefix {
            IpNet::V4(_) => {
                for &idx in indices {
                    assert!(idx < half, "IPv4 entry has index {} >= half {}", idx, half);
                }
            }
            IpNet::V6(_) => {
                for &idx in indices {
                    assert!(idx >= half, "IPv6 entry has index {} < half {}", idx, half);
                }
            }
        }
    }
}

#[test]
fn test_10k_mixed_source_map() {
    run_mixed_source_map(10_000, "10k_mixed_source_map");
}

#[test]
#[cfg_attr(not(feature = "stress"), ignore)]
fn test_100k_mixed_source_map() {
    run_mixed_source_map(100_000, "100k_mixed_source_map");
}

#[test]
#[cfg_attr(not(feature = "stress"), ignore)]
fn test_1m_mixed_source_map() {
    run_mixed_source_map(1_000_000, "1m_mixed_source_map");
}

// --- 8b: Source-map completeness under lossy ---

fn run_source_map_completeness(n: u32, label: &str) {
    let input = generate_contiguous_v4(Ipv4Addr::new(10, 0, 0, 0), n);
    let config = OptimizerConfig {
        ipv4_target: Some(TargetSpec::EntryCount(100)),
        source_map: true,
        max_over_coverage_ratio: None,
        ..Default::default()
    };

    let result = time_it(label, || optimize(&input, &config).unwrap());
    assert!(validate_coverage(&input, &result.entries));

    let mut all_indices: HashSet<usize> = HashSet::new();
    for entry in &result.entries {
        let indices = entry.source_indices.as_ref().expect("source_map must be set");
        for &idx in indices {
            all_indices.insert(idx);
        }
    }
    let expected: HashSet<usize> = (0..n as usize).collect();
    assert_eq!(all_indices, expected, "source-map completeness failed for {}", label);
}

#[test]
fn test_10k_source_map_completeness() {
    run_source_map_completeness(10_000, "10k_source_map_completeness");
}

#[test]
#[cfg_attr(not(feature = "stress"), ignore)]
fn test_100k_source_map_completeness() {
    run_source_map_completeness(100_000, "100k_source_map_completeness");
}

#[test]
#[cfg_attr(not(feature = "stress"), ignore)]
fn test_1m_source_map_completeness() {
    run_source_map_completeness(1_000_000, "1m_source_map_completeness");
}

// --- 8c: Duplicate inputs ---

fn run_duplicates_source_map(n: usize, label: &str) {
    let half = n / 2;
    let net_a: IpNet = "10.0.0.0/24".parse().unwrap();
    let net_b: IpNet = "192.168.0.0/24".parse().unwrap();
    let input: Vec<IpNet> = std::iter::repeat(net_a).take(half)
        .chain(std::iter::repeat(net_b).take(half))
        .collect();

    let config = OptimizerConfig {
        source_map: true,
        ..Default::default()
    };

    let result = time_it(label, || optimize(&input, &config).unwrap());
    assert!(validate_coverage(&input, &result.entries));
    assert_eq!(result.entries.len(), 2, "expected 2 entries for duplicates");

    for entry in &result.entries {
        let indices = entry.source_indices.as_ref().expect("source_map must be set");
        assert_eq!(indices.len(), half, "each entry should have {} source indices", half);
    }
}

#[test]
fn test_10k_duplicates_source_map() {
    run_duplicates_source_map(10_000, "10k_duplicates_source_map");
}

#[test]
#[cfg_attr(not(feature = "stress"), ignore)]
fn test_100k_duplicates_source_map() {
    run_duplicates_source_map(100_000, "100k_duplicates_source_map");
}

#[test]
#[cfg_attr(not(feature = "stress"), ignore)]
fn test_1m_duplicates_source_map() {
    run_duplicates_source_map(1_000_000, "1m_duplicates_source_map");
}
