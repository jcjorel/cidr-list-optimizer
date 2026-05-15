mod common;

use std::net::Ipv6Addr;

use cidr_optimizer::{optimize, validate_coverage, OptimizerConfig};
use ipnet::IpNet;

use common::{generate_contiguous_v6, time_it};

/// Binary decomposition for u128 address space (IPv6 /128s).
/// Same algorithm as minimal_prefix_decomposition_count but for u128.
fn minimal_prefix_decomposition_count_v6(start: u128, count: u64) -> usize {
    if count == 0 {
        return 0;
    }
    let mut remaining = count as u128;
    let mut pos = start;
    let mut blocks = 0usize;
    while remaining > 0 {
        // Largest power-of-2 aligned at pos
        let alignment = if pos == 0 {
            1u128 << 127 // max alignment (use 2^127 as practical max)
        } else {
            pos & pos.wrapping_neg()
        };
        let max_block = alignment.min(remaining);
        // Round down to power of 2
        let block_size = 1u128 << (127 - (max_block.leading_zeros()));
        pos += block_size;
        remaining -= block_size;
        blocks += 1;
    }
    blocks
}

#[test]
fn test_10k_sequential_v6() {
    let base = Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 0);
    let input = generate_contiguous_v6(base, 10_000);
    let start = u128::from(base);
    let expected_count = minimal_prefix_decomposition_count_v6(start, 10_000);

    let result = time_it("10k_sequential_v6", || {
        optimize(&input, &OptimizerConfig::default()).unwrap()
    });

    assert!(validate_coverage(&input, &result.entries));
    assert_eq!(result.entries.len(), expected_count);
    eprintln!("  output entries: {}", result.entries.len());
}

#[test]
fn test_65536_sequential_v6() {
    let base = Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 0);
    let input = generate_contiguous_v6(base, 65_536);

    let result = time_it("65536_sequential_v6", || {
        optimize(&input, &OptimizerConfig::default()).unwrap()
    });

    assert!(validate_coverage(&input, &result.entries));
    assert_eq!(result.entries.len(), 1);
    let expected: IpNet = "2001:db8::/112".parse().unwrap();
    assert_eq!(result.entries[0].prefix, expected);
    eprintln!("  output entries: {}", result.entries.len());
}

#[test]
#[cfg_attr(not(feature = "stress"), ignore)]
fn test_100k_sequential_v6() {
    let base = Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 0);
    let input = generate_contiguous_v6(base, 100_000);
    let start = u128::from(base);
    let expected_count = minimal_prefix_decomposition_count_v6(start, 100_000);

    let result = time_it("100k_sequential_v6", || {
        optimize(&input, &OptimizerConfig::default()).unwrap()
    });

    assert!(validate_coverage(&input, &result.entries));
    assert_eq!(result.entries.len(), expected_count);
    eprintln!("  output entries: {}", result.entries.len());
}

#[test]
#[cfg_attr(not(feature = "stress"), ignore)]
fn test_1m_sequential_v6() {
    let base = Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 0);
    let input = generate_contiguous_v6(base, 1_048_576);

    let result = time_it("1m_sequential_v6", || {
        optimize(&input, &OptimizerConfig::default()).unwrap()
    });

    assert!(validate_coverage(&input, &result.entries));
    assert_eq!(result.entries.len(), 1);
    let expected: IpNet = "2001:db8::/108".parse().unwrap();
    assert_eq!(result.entries[0].prefix, expected);
    eprintln!("  output entries: {}", result.entries.len());
}
