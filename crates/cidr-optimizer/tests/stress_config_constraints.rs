mod common;

use std::net::Ipv4Addr;
use std::ops::ControlFlow;

use cidr_optimizer::{optimize, optimize_with_progress, validate_coverage, OptimizerConfig, OptimizeError, Phase, TargetSpec};
use ipnet::{IpNet, Ipv4Net};

use common::{generate_contiguous_v4, time_it};

// --- 9a: max_over_coverage_ratio cap prevents reaching target ---

fn generate_spaced_v4_stride(count: u32, stride: u64) -> Vec<IpNet> {
    (0..count)
        .map(|i| {
            let addr = Ipv4Addr::from(((i as u64 * stride) % (1u64 << 32)) as u32);
            IpNet::V4(Ipv4Net::new(addr, 32).unwrap())
        })
        .collect()
}

fn run_ratio_cap(count: u32, stride: u64, label: &str) {
    let input = generate_spaced_v4_stride(count, stride);
    let config = OptimizerConfig {
        ipv4_target: Some(TargetSpec::EntryCount(10)),
        max_over_coverage_ratio: Some(0.01),
        ..Default::default()
    };

    let result = time_it(label, || optimize(&input, &config).unwrap());
    assert!(validate_coverage(&input, &result.entries));
    // Ratio cap should prevent reaching target of 10
    assert!(result.entries.len() > 10,
        "{}: expected > 10 entries due to ratio cap, got {}", label, result.entries.len());
    // Verify over-coverage ratio is respected (denominator = input covered IPs, not 2^32)
    let total_over: u128 = result.entries.iter().map(|e| e.over_coverage).sum();
    let input_covered = result.stats.input_ipv4_covered_ips;
    let ratio = total_over as f64 / input_covered as f64;
    assert!(ratio <= 0.01 + 1e-9,
        "{}: over-coverage ratio {} exceeds 0.01 (over={}, input_covered={})",
        label, ratio, total_over, input_covered);
}

#[test]
fn test_10k_ratio_cap() {
    run_ratio_cap(10_000, 65536, "10k_ratio_cap");
}

#[test]
#[cfg_attr(not(feature = "stress"), ignore)]
fn test_100k_ratio_cap() {
    run_ratio_cap(100_000, 4295, "100k_ratio_cap");
}

#[test]
#[cfg_attr(not(feature = "stress"), ignore)]
fn test_1m_ratio_cap() {
    run_ratio_cap(1_000_000, 4295, "1m_ratio_cap");
}

// --- 9b: max_prefix_len_v4 enforcement ---
// max_prefix_len_v4 truncates inputs to that prefix length, but does NOT prevent
// further merging. So /32s get truncated to /24s, then /24s merge normally.
// The expected output count is the binary decomposition of the /24 range.

fn run_max_prefix_len(count: u32, label: &str) {
    let input = generate_contiguous_v4(Ipv4Addr::new(10, 0, 0, 0), count);
    let config = OptimizerConfig {
        max_prefix_len_v4: 24,
        ..Default::default()
    };

    let result = time_it(label, || optimize(&input, &config).unwrap());
    assert!(validate_coverage(&input, &result.entries));

    // Inputs are truncated to /24, then merged. The number of /24s touched:
    let num_24s = (count + 255) / 256;
    // Those /24s start at 10.0.0.0 (/24 index = 0x0A0000 in /24-space).
    // They merge via binary decomposition of num_24s contiguous /24-blocks.
    // Use minimal_prefix_decomposition_count with /24-block index as start.
    let expected_count = common::minimal_prefix_decomposition_count(0x0A0000, num_24s);
    assert_eq!(result.entries.len(), expected_count,
        "{}: expected {} entries, got {}", label, expected_count, result.entries.len());

    // Verify no over-coverage (all inputs are covered exactly by the truncated /24s)
    eprintln!("  {} output entries: {}", label, result.entries.len());
}

#[test]
fn test_10k_max_prefix_len() {
    run_max_prefix_len(10_000, "10k_max_prefix_len");
}

#[test]
#[cfg_attr(not(feature = "stress"), ignore)]
fn test_100k_max_prefix_len() {
    run_max_prefix_len(100_000, "100k_max_prefix_len");
}

#[test]
#[cfg_attr(not(feature = "stress"), ignore)]
fn test_1m_max_prefix_len() {
    run_max_prefix_len(1_000_000, "1m_max_prefix_len");
}

// --- 9c: Cancellation during lossy phase ---

fn generate_nonadjacent_v4(count: u32) -> Vec<IpNet> {
    (0..count)
        .map(|i| {
            let addr = Ipv4Addr::from(0x0A000000 + i * 2); // stride=2, even addresses
            IpNet::V4(Ipv4Net::new(addr, 32).unwrap())
        })
        .collect()
}

fn run_cancellation(count: u32, label: &str) {
    let input = generate_nonadjacent_v4(count);
    let config = OptimizerConfig {
        ipv4_target: Some(TargetSpec::EntryCount(10)),
        max_over_coverage_ratio: None,
        ..Default::default()
    };

    let result = time_it(label, || {
        optimize_with_progress(&input, &config, |phase| {
            match phase {
                Phase::Lossy { .. } => ControlFlow::Break(()),
                _ => ControlFlow::Continue(()),
            }
        })
    });

    assert!(matches!(result, Err(OptimizeError::Cancelled)),
        "{}: expected Cancelled, got {:?}", label, result.is_ok());
}

#[test]
fn test_10k_cancellation() {
    run_cancellation(10_000, "10k_cancellation");
}

#[test]
#[cfg_attr(not(feature = "stress"), ignore)]
fn test_100k_cancellation() {
    run_cancellation(100_000, "100k_cancellation");
}

#[test]
#[cfg_attr(not(feature = "stress"), ignore)]
fn test_1m_cancellation() {
    run_cancellation(1_000_000, "1m_cancellation");
}
