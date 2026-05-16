pub mod types;
pub mod error;
pub mod parser;
pub mod lossless;
pub mod trie;
pub mod optimizer;
pub mod source_map;
pub mod exclusion;

pub use types::{
    AddressFamily, AggregatedEntry, ExclusionCollision, ExclusionEntry, InputEntry,
    OptimizerConfig, OptimizationResult, OptimizationStats, Phase, ReaderResult, TargetSpec,
};
pub use error::{OptimizeError, OptimizerError};

use std::io::BufRead;
use std::ops::ControlFlow;

use ipnet::{IpNet, Ipv4Net, Ipv6Net};

use crate::exclusion::ExclusionSet;
use crate::lossless::SourceMapPrefix;

/// Primary API: optimize from pre-parsed prefixes.
pub fn optimize(
    prefixes: &[IpNet],
    config: &OptimizerConfig,
) -> Result<OptimizationResult, OptimizeError> {
    optimize_with_progress(prefixes, config, |_| ControlFlow::Continue(()))
}

/// With progress callback and cancellation support.
pub fn optimize_with_progress(
    prefixes: &[IpNet],
    config: &OptimizerConfig,
    mut progress: impl FnMut(Phase) -> ControlFlow<()>,
) -> Result<OptimizationResult, OptimizeError> {
    if prefixes.len() > config.max_input_entries {
        return Err(OptimizeError::InputTooLarge {
            count: prefixes.len(),
            limit: config.max_input_entries,
        });
    }

    validate_config(config)?;

    let (ipv4, ipv6) = partition_with_indices(prefixes);

    if ipv4.is_empty() && ipv6.is_empty() {
        return Err(OptimizeError::EmptyInput);
    }

    if let Some(TargetSpec::EntryCount(0)) = config.ipv4_target {
        if !ipv4.is_empty() {
            return Err(OptimizeError::TargetTooSmall { target: 0, minimum: 1 });
        }
    }
    if let Some(TargetSpec::EntryCount(0)) = config.ipv6_target {
        if !ipv6.is_empty() {
            return Err(OptimizeError::TargetTooSmall { target: 0, minimum: 1 });
        }
    }

    let input_ipv4_count = ipv4.len();
    let input_ipv6_count = ipv6.len();

    if progress(Phase::Lossless { af: AddressFamily::IPv4, entries_remaining: ipv4.len() }) == ControlFlow::Break(()) {
        return Err(OptimizeError::Cancelled);
    }
    let lossless_v4 = lossless::lossless_aggregate_v4(ipv4, config.max_prefix_len_v4);

    if progress(Phase::Lossless { af: AddressFamily::IPv6, entries_remaining: ipv6.len() }) == ControlFlow::Break(()) {
        return Err(OptimizeError::Cancelled);
    }
    let lossless_v6 = lossless::lossless_aggregate_v6(ipv6, config.max_prefix_len_v6);

    let ipv4_target_binding = matches!(config.ipv4_target, Some(TargetSpec::EntryCount(t)) if lossless_v4.len() > t);
    let ipv6_target_binding = matches!(config.ipv6_target, Some(TargetSpec::EntryCount(t)) if lossless_v6.len() > t);

    // Build exclusion set from config
    let exclusion_set = ExclusionSet::build(&config.exclusions);

    let output_v4 = match config.ipv4_target {
        Some(TargetSpec::EntryCount(target)) if lossless_v4.len() > target => {
            let input_ips = compute_covered_ips_v4(&lossless_v4);
            if progress(Phase::Lossy { af: AddressFamily::IPv4, current_count: lossless_v4.len(), target }) == ControlFlow::Break(()) {
                return Err(OptimizeError::Cancelled);
            }
            lossy_optimize_v4(&lossless_v4, target, config.max_over_coverage_ratio, input_ips, &exclusion_set)?
        }
        Some(TargetSpec::MaxOverCoverage(ratio)) if lossless_v4.len() > 1 => {
            let input_ips = compute_covered_ips_v4(&lossless_v4);
            if progress(Phase::Lossy { af: AddressFamily::IPv4, current_count: lossless_v4.len(), target: 1 }) == ControlFlow::Break(()) {
                return Err(OptimizeError::Cancelled);
            }
            lossy_optimize_v4(&lossless_v4, 1, Some(ratio), input_ips, &exclusion_set)?
        }
        _ => lossless_v4,
    };

    let output_v6 = match config.ipv6_target {
        Some(TargetSpec::EntryCount(target)) if lossless_v6.len() > target => {
            let input_ips = compute_covered_ips_v6(&lossless_v6);
            if progress(Phase::Lossy { af: AddressFamily::IPv6, current_count: lossless_v6.len(), target }) == ControlFlow::Break(()) {
                return Err(OptimizeError::Cancelled);
            }
            lossy_optimize_v6(&lossless_v6, target, config.max_over_coverage_ratio, input_ips, &exclusion_set)?
        }
        Some(TargetSpec::MaxOverCoverage(ratio)) if lossless_v6.len() > 1 => {
            let input_ips = compute_covered_ips_v6(&lossless_v6);
            if progress(Phase::Lossy { af: AddressFamily::IPv6, current_count: lossless_v6.len(), target: 1 }) == ControlFlow::Break(()) {
                return Err(OptimizeError::Cancelled);
            }
            lossy_optimize_v6(&lossless_v6, 1, Some(ratio), input_ips, &exclusion_set)?
        }
        _ => lossless_v6,
    };

    let _ = progress(Phase::Done);

    // Detect exclusion-constrained: target was set but not met, and exclusions are active
    let ipv4_exclusion_constrained = match config.ipv4_target {
        Some(TargetSpec::EntryCount(target)) => {
            output_v4.len() > target && !exclusion_set.is_empty_v4()
        }
        _ => false,
    };
    let ipv6_exclusion_constrained = match config.ipv6_target {
        Some(TargetSpec::EntryCount(target)) => {
            output_v6.len() > target && !exclusion_set.is_empty_v6()
        }
        _ => false,
    };

    let mut result = build_result(
        output_v4, output_v6, input_ipv4_count, input_ipv6_count,
        ipv4_target_binding, ipv6_target_binding,
        ipv4_exclusion_constrained, ipv6_exclusion_constrained,
    )?;

    // Detect input/exclusion collisions
    if !config.exclusions.is_empty() {
        for entry in &mut result.entries {
            let (start, end) = prefix_to_interval(&entry.prefix);
            let collisions: Vec<ExclusionCollision> = match &entry.prefix {
                IpNet::V4(_) => exclusion_set
                    .find_intersecting_v4(&config.exclusions, start, end)
                    .into_iter()
                    .map(|e| ExclusionCollision {
                        exclusion_prefix: e.prefix.to_string(),
                        exclusion_source: e.source.clone(),
                        exclusion_comment: e.comment.clone(),
                    })
                    .collect(),
                IpNet::V6(_) => exclusion_set
                    .find_intersecting_v6(&config.exclusions, start, end)
                    .into_iter()
                    .map(|e| ExclusionCollision {
                        exclusion_prefix: e.prefix.to_string(),
                        exclusion_source: e.source.clone(),
                        exclusion_comment: e.comment.clone(),
                    })
                    .collect(),
            };
            if !collisions.is_empty() {
                entry.exclusion_collisions = Some(collisions);
            }
        }
    }

    // Populate source-map via binary search when enabled
    if config.source_map {
        let (sorted_v4, sorted_v6) = partition_with_indices(prefixes);
        let mut sorted_v4 = sorted_v4;
        let mut sorted_v6 = sorted_v6;
        sorted_v4.sort_by_key(|(_, p)| u32::from(p.network()));
        sorted_v6.sort_by_key(|(_, p)| u128::from(p.network()));

        let output_v4_nets: Vec<Ipv4Net> = result.entries.iter()
            .filter_map(|e| if let IpNet::V4(v4) = e.prefix { Some(v4) } else { None })
            .collect();
        let output_v6_nets: Vec<Ipv6Net> = result.entries.iter()
            .filter_map(|e| if let IpNet::V6(v6) = e.prefix { Some(v6) } else { None })
            .collect();

        let prov_v4 = source_map::compute_source_map_v4(&output_v4_nets, &sorted_v4);
        let prov_v6 = source_map::compute_source_map_v6(&output_v6_nets, &sorted_v6);

        let mut v4_idx = 0;
        let mut v6_idx = 0;
        for entry in &mut result.entries {
            match entry.prefix {
                IpNet::V4(_) => {
                    if v4_idx < prov_v4.len() {
                        entry.source_indices = Some(prov_v4[v4_idx].clone());
                        v4_idx += 1;
                    }
                }
                IpNet::V6(_) => {
                    if v6_idx < prov_v6.len() {
                        entry.source_indices = Some(prov_v6[v6_idx].clone());
                        v6_idx += 1;
                    }
                }
            }
        }
    }

    // Safety invariant: output MUST cover all input prefixes
    if !validate_coverage(prefixes, &result.entries) {
        return Err(OptimizeError::CoverageLost);
    }

    Ok(result)
}

/// Convenience: parse from reader then optimize.
pub fn optimize_from_reader(
    input: impl BufRead,
    config: &OptimizerConfig,
) -> Result<ReaderResult, OptimizerError> {
    let parsed = parser::parse_input(input, config.source_map, config.max_input_entries)?;
    let prefixes: Vec<IpNet> = parsed
        .ipv4
        .iter()
        .map(|(_, p)| IpNet::V4(*p))
        .chain(parsed.ipv6.iter().map(|(_, p)| IpNet::V6(*p)))
        .collect();
    if prefixes.is_empty() {
        return Err(OptimizeError::EmptyInput.into());
    }
    let result = optimize(&prefixes, config)?;
    Ok(ReaderResult {
        result,
        input_metadata: parsed.input_metadata,
    })
}

fn validate_config(config: &OptimizerConfig) -> Result<(), OptimizeError> {
    if config.max_prefix_len_v4 == 0 || config.max_prefix_len_v4 > 32 {
        return Err(OptimizeError::InvalidConfig {
            message: format!("max_prefix_len_v4 ({}) must be in [1, 32]", config.max_prefix_len_v4),
        });
    }
    if config.max_prefix_len_v6 == 0 || config.max_prefix_len_v6 > 128 {
        return Err(OptimizeError::InvalidConfig {
            message: format!("max_prefix_len_v6 ({}) must be in [1, 128]", config.max_prefix_len_v6),
        });
    }
    if let Some(r) = config.max_over_coverage_ratio {
        if !(0.0..=10.0).contains(&r) {
            return Err(OptimizeError::InvalidConfig {
                message: format!("max_over_coverage_ratio ({}) must be in [0.0, 10.0] (0-1000%)", r),
            });
        }
    }
    // Validate TargetSpec::MaxOverCoverage ratios
    for (label, target) in [("ipv4_target", &config.ipv4_target), ("ipv6_target", &config.ipv6_target)] {
        if let Some(TargetSpec::MaxOverCoverage(ratio)) = target {
            if *ratio <= 0.0 || *ratio > 10.0 {
                return Err(OptimizeError::InvalidConfig {
                    message: format!("{} MaxOverCoverage ratio ({}) must be in (0.0, 10.0]", label, ratio),
                });
            }
            // Conflict: MaxOverCoverage target + max_over_coverage_ratio
            if config.max_over_coverage_ratio.is_some() {
                return Err(OptimizeError::InvalidConfig {
                    message: format!("{} is MaxOverCoverage — cannot also set max_over_coverage_ratio", label),
                });
            }
        }
    }
    Ok(())
}

#[allow(clippy::type_complexity)]
fn partition_with_indices(prefixes: &[IpNet]) -> (Vec<(usize, Ipv4Net)>, Vec<(usize, Ipv6Net)>) {
    let mut ipv4 = Vec::new();
    let mut ipv6 = Vec::new();
    for (i, p) in prefixes.iter().enumerate() {
        match p {
            IpNet::V4(v4) => ipv4.push((i, v4.trunc())),
            IpNet::V6(v6) => ipv6.push((i, v6.trunc())),
        }
    }
    (ipv4, ipv6)
}

fn compute_covered_ips_v4(lossless: &[SourceMapPrefix<Ipv4Net>]) -> u128 {
    lossless.iter().map(|e| {
        let pl = e.prefix.prefix_len();
        if pl == 32 { 1u128 } else { 1u128 << (32 - pl) }
    }).sum()
}

fn compute_covered_ips_v6(lossless: &[SourceMapPrefix<Ipv6Net>]) -> u128 {
    lossless.iter().map(|e| {
        let pl = e.prefix.prefix_len();
        if pl == 128 { 1u128 } else if (128 - pl) >= 128 { u128::MAX } else { 1u128 << (128 - pl) }
    }).fold(0u128, |acc, v| acc.saturating_add(v))
}

fn lossy_optimize_v4(
    lossless: &[SourceMapPrefix<Ipv4Net>],
    target: usize,
    max_ratio: Option<f64>,
    input_covered_ips: u128,
    exclusion_set: &ExclusionSet,
) -> Result<Vec<SourceMapPrefix<Ipv4Net>>, OptimizeError> {
    let mut trie = trie::BinaryTrie::build_from_v4(lossless)?;
    if !exclusion_set.is_empty_v4() {
        trie.mark_exclusions(exclusion_set, true);
    }
    let _leaf_indices = optimizer::optimize_trie(&mut trie, target, max_ratio, input_covered_ips);
    Ok(trie.extract_leaves_v4())
}

fn lossy_optimize_v6(
    lossless: &[SourceMapPrefix<Ipv6Net>],
    target: usize,
    max_ratio: Option<f64>,
    input_covered_ips: u128,
    exclusion_set: &ExclusionSet,
) -> Result<Vec<SourceMapPrefix<Ipv6Net>>, OptimizeError> {
    let mut trie = trie::BinaryTrie::build_from_v6(lossless)?;
    if !exclusion_set.is_empty_v6() {
        trie.mark_exclusions(exclusion_set, false);
    }
    let _leaf_indices = optimizer::optimize_trie(&mut trie, target, max_ratio, input_covered_ips);
    Ok(trie.extract_leaves_v6())
}

#[allow(clippy::too_many_arguments)]
fn build_result(
    output_v4: Vec<SourceMapPrefix<Ipv4Net>>,
    output_v6: Vec<SourceMapPrefix<Ipv6Net>>,
    input_ipv4_count: usize,
    input_ipv6_count: usize,
    ipv4_target_binding: bool,
    ipv6_target_binding: bool,
    ipv4_exclusion_constrained: bool,
    ipv6_exclusion_constrained: bool,
) -> Result<OptimizationResult, OptimizeError> {
    let output_ipv4_count = output_v4.len();
    let output_ipv6_count = output_v6.len();

    let total_ipv4_over_coverage: u128 = output_v4.iter().map(|e| {
        let cap = if e.prefix.prefix_len() == 32 { 1u128 } else { 1u128 << (32 - e.prefix.prefix_len()) };
        cap.saturating_sub(e.coverage)
    }).sum();

    let total_ipv6_over_coverage: u128 = output_v6.iter().map(|e| {
        let pl = e.prefix.prefix_len();
        let cap = if pl == 128 { 1u128 } else if (128 - pl) >= 128 { u128::MAX } else { 1u128 << (128 - pl) };
        cap.saturating_sub(e.coverage)
    }).sum();

    let ipv4_compression_ratio = if input_ipv4_count > 0 {
        input_ipv4_count as f64 / output_ipv4_count.max(1) as f64
    } else {
        1.0
    };
    let ipv6_compression_ratio = if input_ipv6_count > 0 {
        input_ipv6_count as f64 / output_ipv6_count.max(1) as f64
    } else {
        1.0
    };

    let mut entries: Vec<AggregatedEntry> = Vec::with_capacity(output_ipv4_count + output_ipv6_count);

    for e in output_v4 {
        let cap = if e.prefix.prefix_len() == 32 { 1u128 } else { 1u128 << (32 - e.prefix.prefix_len()) };
        entries.push(AggregatedEntry {
            prefix: IpNet::V4(e.prefix),
            source_indices: if e.source_indices.is_empty() { None } else { Some(e.source_indices) },
            over_coverage: cap.saturating_sub(e.coverage),
            exclusion_collisions: None,
        });
    }
    for e in output_v6 {
        let pl = e.prefix.prefix_len();
        let cap = if pl == 128 { 1u128 } else if (128 - pl) >= 128 { u128::MAX } else { 1u128 << (128 - pl) };
        entries.push(AggregatedEntry {
            prefix: IpNet::V6(e.prefix),
            source_indices: if e.source_indices.is_empty() { None } else { Some(e.source_indices) },
            over_coverage: cap.saturating_sub(e.coverage),
            exclusion_collisions: None,
        });
    }

    // Sort: IPv4 first by network addr, then IPv6 by network addr
    entries.sort_by(|a, b| match (&a.prefix, &b.prefix) {
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

    Ok(OptimizationResult {
        entries,
        stats: OptimizationStats {
            input_ipv4_count,
            input_ipv6_count,
            output_ipv4_count,
            output_ipv6_count,
            total_ipv4_over_coverage,
            total_ipv6_over_coverage,
            ipv4_compression_ratio,
            ipv6_compression_ratio,
            ipv4_target_binding,
            ipv6_target_binding,
            ipv4_exclusion_constrained,
            ipv6_exclusion_constrained,
        },
    })
}

/// Convert a prefix to its (start, end) interval for collision detection.
fn prefix_to_interval(prefix: &IpNet) -> (u128, u128) {
    match prefix {
        IpNet::V4(v4) => {
            let start = u32::from(v4.network()) as u128;
            let end = u32::from(v4.broadcast()) as u128;
            (start, end)
        }
        IpNet::V6(v6) => {
            let start = u128::from(v6.network());
            let pl = v6.prefix_len();
            let end = if pl == 128 {
                start
            } else if pl == 0 {
                u128::MAX
            } else {
                start | ((1u128 << (128 - pl)) - 1)
            };
            (start, end)
        }
    }
}

/// Verify that every input prefix is contained by at least one output prefix.
/// Uses O(N log M) binary search: output is sorted by network address,
/// so we binary-search for candidates and scan backwards for containment.
pub fn validate_coverage(input: &[IpNet], output: &[AggregatedEntry]) -> bool {
    if output.is_empty() {
        return input.is_empty();
    }

    // Separate output into sorted v4 and v6 lists (by network address)
    let mut v4_prefixes: Vec<Ipv4Net> = Vec::new();
    let mut v6_prefixes: Vec<Ipv6Net> = Vec::new();
    for entry in output {
        match entry.prefix {
            IpNet::V4(v4) => v4_prefixes.push(v4),
            IpNet::V6(v6) => v6_prefixes.push(v6),
        }
    }
    v4_prefixes.sort_by_key(|p| (u32::from(p.network()), p.prefix_len()));
    v6_prefixes.sort_by_key(|p| (u128::from(p.network()), p.prefix_len()));

    input.iter().all(|inp| match inp {
        IpNet::V4(v4) => is_covered_v4(*v4, &v4_prefixes),
        IpNet::V6(v6) => is_covered_v6(*v6, &v6_prefixes),
    })
}

/// Check if `needle` is contained by any prefix in the sorted list.
/// Binary search for the insertion point, then scan backwards checking containment.
fn is_covered_v4(needle: Ipv4Net, sorted: &[Ipv4Net]) -> bool {
    let needle_net = u32::from(needle.network());
    let needle_bcast = u32::from(needle.broadcast());

    // Find rightmost entry whose network <= needle's network
    let idx = sorted.partition_point(|p| u32::from(p.network()) <= needle_net);
    // Check candidates backwards from idx-1
    for i in (0..idx).rev() {
        let p = &sorted[i];
        let p_net = u32::from(p.network());
        let p_bcast = u32::from(p.broadcast());
        if p_net <= needle_net && p_bcast >= needle_bcast {
            return true;
        }
        // If this prefix's broadcast is less than needle's network,
        // no earlier prefix can contain needle either
        if p_bcast < needle_net {
            break;
        }
    }
    false
}

/// Check if `needle` is contained by any prefix in the sorted list (IPv6 version).
fn is_covered_v6(needle: Ipv6Net, sorted: &[Ipv6Net]) -> bool {
    let needle_net = u128::from(needle.network());
    let needle_bcast = u128::from(needle.broadcast());

    let idx = sorted.partition_point(|p| u128::from(p.network()) <= needle_net);
    for i in (0..idx).rev() {
        let p = &sorted[i];
        let p_net = u128::from(p.network());
        let p_bcast = u128::from(p.broadcast());
        if p_net <= needle_net && p_bcast >= needle_bcast {
            return true;
        }
        if p_bcast < needle_net {
            break;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optimize_lossless_only() {
        let prefixes: Vec<IpNet> = vec![
            "10.0.0.0/25".parse().unwrap(),
            "10.0.0.128/25".parse().unwrap(),
        ];
        let config = OptimizerConfig::default();
        let result = optimize(&prefixes, &config).unwrap();
        // Lossless merges siblings → 10.0.0.0/24
        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.stats.output_ipv4_count, 1);
        assert_eq!(result.stats.total_ipv4_over_coverage, 0);
    }

    #[test]
    fn optimize_with_target() {
        // Use non-sibling prefixes so lossless doesn't merge them all
        let prefixes: Vec<IpNet> = vec![
            "10.0.0.0/24".parse().unwrap(),
            "10.0.1.0/24".parse().unwrap(),
            "10.0.4.0/24".parse().unwrap(),
            "10.0.5.0/24".parse().unwrap(),
        ];
        let config = OptimizerConfig {
            ipv4_target: Some(TargetSpec::EntryCount(1)),
            ..Default::default()
        };
        let result = optimize(&prefixes, &config).unwrap();
        assert!(result.entries.len() <= 1);
        assert!(result.stats.ipv4_target_binding);
    }

    #[test]
    fn optimize_target_not_binding() {
        let prefixes: Vec<IpNet> = vec![
            "10.0.0.0/25".parse().unwrap(),
            "10.0.0.128/25".parse().unwrap(),
        ];
        let config = OptimizerConfig {
            ipv4_target: Some(TargetSpec::EntryCount(10)),
            ..Default::default()
        };
        let result = optimize(&prefixes, &config).unwrap();
        // Lossless produces 1 entry, target=10 is not binding
        assert!(!result.stats.ipv4_target_binding);
    }

    #[test]
    fn optimize_empty_input_error() {
        let prefixes: Vec<IpNet> = vec![];
        let config = OptimizerConfig::default();
        let result = optimize(&prefixes, &config);
        assert!(matches!(result, Err(OptimizeError::EmptyInput)));
    }

    #[test]
    fn optimize_target_zero_error() {
        let prefixes: Vec<IpNet> = vec!["10.0.0.0/24".parse().unwrap()];
        let config = OptimizerConfig {
            ipv4_target: Some(TargetSpec::EntryCount(0)),
            ..Default::default()
        };
        let result = optimize(&prefixes, &config);
        assert!(matches!(result, Err(OptimizeError::TargetTooSmall { .. })));
    }

    #[test]
    fn optimize_no_target_means_lossless() {
        // Use non-sibling prefixes that won't merge
        let prefixes: Vec<IpNet> = vec![
            "10.0.0.0/24".parse().unwrap(),
            "10.0.2.0/24".parse().unwrap(),
            "10.0.4.0/24".parse().unwrap(),
        ];
        let config = OptimizerConfig::default();
        let result = optimize(&prefixes, &config).unwrap();
        // No target → lossless → 3 entries (non-siblings)
        assert_eq!(result.entries.len(), 3);
        assert_eq!(result.stats.total_ipv4_over_coverage, 0);
    }

    #[test]
    fn optimize_max_over_coverage_stops_at_ratio() {
        // 4 scattered /32s — merging all to /30 would give 4 IPs capacity, 0 original = 4 over-coverage
        // With ratio=1.0 (100%), over-coverage must stay ≤ input_ips
        let prefixes: Vec<IpNet> = vec![
            "10.0.0.0/32".parse().unwrap(),
            "10.0.0.1/32".parse().unwrap(),
            "10.0.0.2/32".parse().unwrap(),
            "10.0.0.3/32".parse().unwrap(),
        ];
        let config = OptimizerConfig {
            ipv4_target: Some(TargetSpec::MaxOverCoverage(1.0)),
            ..Default::default()
        };
        let result = optimize(&prefixes, &config).unwrap();
        // Should merge but respect ratio — result count depends on greedy loop
        // Key assertion: over-coverage ≤ input_ips (4 IPs * 1.0 = 4)
        assert!(result.stats.total_ipv4_over_coverage <= 4);
        // target_binding is always false for MaxOverCoverage
        assert!(!result.stats.ipv4_target_binding);
    }

    #[test]
    fn optimize_max_over_coverage_conflict_with_ratio() {
        let prefixes: Vec<IpNet> = vec!["10.0.0.0/24".parse().unwrap()];
        let config = OptimizerConfig {
            ipv4_target: Some(TargetSpec::MaxOverCoverage(0.5)),
            max_over_coverage_ratio: Some(1.0),
            ..Default::default()
        };
        let result = optimize(&prefixes, &config);
        assert!(matches!(result, Err(OptimizeError::InvalidConfig { .. })));
    }

    #[test]
    fn optimize_max_over_coverage_zero_rejected() {
        let prefixes: Vec<IpNet> = vec!["10.0.0.0/24".parse().unwrap()];
        let config = OptimizerConfig {
            ipv4_target: Some(TargetSpec::MaxOverCoverage(0.0)),
            ..Default::default()
        };
        let result = optimize(&prefixes, &config);
        assert!(matches!(result, Err(OptimizeError::InvalidConfig { .. })));
    }

    #[test]
    fn optimize_entry_count_still_works() {
        let prefixes: Vec<IpNet> = vec![
            "10.0.0.0/24".parse().unwrap(),
            "10.0.2.0/24".parse().unwrap(),
            "10.0.4.0/24".parse().unwrap(),
            "10.0.6.0/24".parse().unwrap(),
        ];
        let config = OptimizerConfig {
            ipv4_target: Some(TargetSpec::EntryCount(2)),
            max_over_coverage_ratio: Some(10.0),
            ..Default::default()
        };
        let result = optimize(&prefixes, &config).unwrap();
        assert!(result.entries.len() <= 2);
        assert!(result.stats.ipv4_target_binding);
    }
}
