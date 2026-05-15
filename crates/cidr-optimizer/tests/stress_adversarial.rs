mod common;

use std::net::Ipv4Addr;

use cidr_optimizer::{optimize, validate_coverage, OptimizerConfig, TargetSpec};
use ipnet::IpNet;

use common::time_it;

/// 6a: All 65536 /32s in 10.0.0.0/16 → dense cascade to single /16
#[test]
fn test_dense_cascade() {
    let base: u32 = 0x0A000000; // 10.0.0.0
    let input: Vec<IpNet> = (0..65_536u32)
        .map(|i| {
            let addr = Ipv4Addr::from(base + i);
            IpNet::V4(ipnet::Ipv4Net::new(addr, 32).unwrap())
        })
        .collect();

    let result = time_it("adversarial_dense_cascade", || {
        optimize(&input, &OptimizerConfig::default()).unwrap()
    });

    assert!(validate_coverage(&input, &result.entries));
    assert_eq!(result.entries.len(), 1);
    let expected: IpNet = "10.0.0.0/16".parse().unwrap();
    assert_eq!(result.entries[0].prefix, expected);
    eprintln!("  output entries: {}", result.entries.len());
}

/// 6b: Alternating bit pattern — even in 10.0.0.x, odd in 10.0.1.x → no merges
#[test]
fn test_alternating_no_merge() {
    let mut input: Vec<IpNet> = Vec::with_capacity(256);
    // 128 even addresses in 10.0.0.x
    for i in 0..128u8 {
        let addr = Ipv4Addr::new(10, 0, 0, i * 2);
        input.push(IpNet::V4(ipnet::Ipv4Net::new(addr, 32).unwrap()));
    }
    // 128 odd addresses in 10.0.1.x
    for i in 0..128u8 {
        let addr = Ipv4Addr::new(10, 0, 1, i * 2 + 1);
        input.push(IpNet::V4(ipnet::Ipv4Net::new(addr, 32).unwrap()));
    }

    let result = time_it("adversarial_alternating_no_merge", || {
        optimize(&input, &OptimizerConfig::default()).unwrap()
    });

    assert!(validate_coverage(&input, &result.entries));
    assert_eq!(result.entries.len(), 256);
    eprintln!("  output entries: {}", result.entries.len());
}

/// 6c: Maximum redundancy — /8 + all /16s + all /24s within it
#[test]
fn test_maximum_redundancy() {
    let mut input: Vec<IpNet> = Vec::with_capacity(65_793);
    // The /8
    input.push("10.0.0.0/8".parse().unwrap());
    // All 256 /16s within 10.0.0.0/8
    for i in 0..256u32 {
        let addr = Ipv4Addr::from(0x0A000000 + (i << 16));
        input.push(IpNet::V4(ipnet::Ipv4Net::new(addr, 16).unwrap()));
    }
    // All 65536 /24s within 10.0.0.0/8
    for i in 0..65_536u32 {
        let addr = Ipv4Addr::from(0x0A000000 + (i << 8));
        input.push(IpNet::V4(ipnet::Ipv4Net::new(addr, 24).unwrap()));
    }

    let result = time_it("adversarial_maximum_redundancy", || {
        optimize(&input, &OptimizerConfig::default()).unwrap()
    });

    assert!(validate_coverage(&input, &result.entries));
    assert_eq!(result.entries.len(), 1);
    let expected: IpNet = "10.0.0.0/8".parse().unwrap();
    assert_eq!(result.entries[0].prefix, expected);
    eprintln!("  output entries: {}", result.entries.len());
}

/// 6d: Sibling /25 pairs at non-adjacent /24 positions → merge to /24 but no cascade
#[test]
fn test_sibling_pairs_no_cascade() {
    let mut input: Vec<IpNet> = Vec::with_capacity(10_000);
    for i in 0..5000u32 {
        let idx = i * 2; // /24 indices: 0, 2, 4, ... (stride=2)
        let octet2 = (idx / 256) as u8;
        let octet3 = (idx % 256) as u8;
        // First /25: 10.{octet2}.{octet3}.0/25
        let addr1 = Ipv4Addr::new(10, octet2, octet3, 0);
        input.push(IpNet::V4(ipnet::Ipv4Net::new(addr1, 25).unwrap()));
        // Second /25: 10.{octet2}.{octet3}.128/25
        let addr2 = Ipv4Addr::new(10, octet2, octet3, 128);
        input.push(IpNet::V4(ipnet::Ipv4Net::new(addr2, 25).unwrap()));
    }

    let result = time_it("adversarial_sibling_pairs_no_cascade", || {
        optimize(&input, &OptimizerConfig::default()).unwrap()
    });

    assert!(validate_coverage(&input, &result.entries));
    assert_eq!(result.entries.len(), 5000);
    eprintln!("  output entries: {}", result.entries.len());
}

/// 6e: Aggressive lossy collapse — 10000 contiguous /32s with target=1
#[test]
fn test_aggressive_lossy_collapse() {
    let base: u32 = 0x0A000000; // 10.0.0.0
    let input: Vec<IpNet> = (0..10_000u32)
        .map(|i| {
            let addr = Ipv4Addr::from(base + i);
            IpNet::V4(ipnet::Ipv4Net::new(addr, 32).unwrap())
        })
        .collect();
    let config = OptimizerConfig {
        ipv4_target: Some(TargetSpec::EntryCount(1)),
        max_over_coverage_ratio: None,
        ..Default::default()
    };

    let result = time_it("adversarial_aggressive_lossy_collapse", || {
        optimize(&input, &config).unwrap()
    });

    assert!(validate_coverage(&input, &result.entries));
    assert_eq!(result.entries.len(), 1);
    let expected: IpNet = "10.0.0.0/18".parse().unwrap();
    assert_eq!(result.entries[0].prefix, expected);
    eprintln!("  output entries: {}", result.entries.len());
}

/// 6f: Single entry edge cases
#[test]
fn test_single_entry_edge_cases() {
    // (a) Single /32, no target
    {
        let input: Vec<IpNet> = vec!["10.0.0.1/32".parse().unwrap()];
        let result = time_it("adversarial_single_v4_no_target", || {
            optimize(&input, &OptimizerConfig::default()).unwrap()
        });
        assert!(validate_coverage(&input, &result.entries));
        assert_eq!(result.entries.len(), 1);
    }

    // (b) Single /32, target=1
    {
        let input: Vec<IpNet> = vec!["10.0.0.1/32".parse().unwrap()];
        let config = OptimizerConfig {
            ipv4_target: Some(TargetSpec::EntryCount(1)),
            ..Default::default()
        };
        let result = time_it("adversarial_single_v4_target_1", || {
            optimize(&input, &config).unwrap()
        });
        assert!(validate_coverage(&input, &result.entries));
        assert_eq!(result.entries.len(), 1);
    }

    // (c) Single IPv6 /128, provenance=true
    {
        let input: Vec<IpNet> = vec!["2001:db8::1/128".parse().unwrap()];
        let config = OptimizerConfig {
            provenance: true,
            ..Default::default()
        };
        let result = time_it("adversarial_single_v6_provenance", || {
            optimize(&input, &config).unwrap()
        });
        assert!(validate_coverage(&input, &result.entries));
        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.entries[0].source_indices, Some(vec![0]));
    }

    eprintln!("  all single-entry edge cases passed");
}
