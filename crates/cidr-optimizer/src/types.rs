//! Core type definitions for the CIDR optimizer: configuration, parsed inputs,
//! optimization results, and source-map tracking structures.

use std::str::FromStr;

use ipnet::IpNet;

use crate::error::OptimizerError;

/// Specifies how the optimization target is defined per address family.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TargetSpec {
    /// Fixed entry count target (existing behavior).
    EntryCount(usize),
    /// Find minimum entries keeping over-coverage ≤ ratio (e.g., 0.001 for 0.1%).
    MaxOverCoverage(f64),
}

impl FromStr for TargetSpec {
    type Err = OptimizerError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Handle over-coverage target: user specifies a max percentage, stored internally as a decimal ratio
        if let Some(rest) = s.strip_prefix("over-coverage=") {
            // Strip the trailing '%', parse the numeric portion, and convert to a decimal ratio
            let rest = rest.strip_suffix('%').ok_or_else(|| OptimizerError::TargetSpecParse {
                message: "over-coverage value must end with '%', e.g. over-coverage=0.1%".into(),
            })?;
            let pct: f64 = rest.parse().map_err(|_| OptimizerError::TargetSpecParse {
                message: format!("invalid over-coverage percentage: '{}'", rest),
            })?;
            // Negative over-coverage is meaningless — it would imply the output covers fewer IPs than the input
            if pct < 0.0 {
                return Err(OptimizerError::TargetSpecParse {
                    message: format!("over-coverage percentage must be non-negative, got '{}'", pct),
                });
            }
            Ok(TargetSpec::MaxOverCoverage(pct / 100.0))
        // Otherwise interpret as a fixed entry-count budget
        } else {
            let n: usize = s.parse().map_err(|_| OptimizerError::TargetSpecParse {
                message: format!("invalid target: '{}' (expected integer or over-coverage=X%)", s),
            })?;
            Ok(TargetSpec::EntryCount(n))
        }
    }
}

/// A single CIDR entry as parsed from input, retaining source location and inline comment for source-map tracking.
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedCidr {
    pub prefix: IpNet,
    pub raw_text: String,
    pub comment: Option<String>,
    pub line_number: usize,
}

/// A single exclusion entry: a prefix that must not appear as over-coverage.
#[derive(Clone, Debug, PartialEq)]
pub struct ExclusionEntry {
    pub prefix: IpNet,
    pub source: String,
    pub comment: Option<String>,
}

/// A single preferred over-coverage entry: widening into this space is discounted.
#[derive(Clone, Debug, PartialEq)]
pub struct PreferredEntry {
    pub prefix: IpNet,
    pub source: String,
    pub comment: Option<String>,
}

/// Records which preferred CIDR contributed to a widening discount (for source-map).
#[derive(Clone, Debug, PartialEq)]
pub struct PreferredContribution {
    pub prefix: String,
    pub source: String,
    pub comment: Option<String>,
}

/// Records a collision between an input prefix and an exclusion entry.
#[derive(Clone, Debug, PartialEq)]
pub struct ExclusionCollision {
    pub exclusion_prefix: String,
    pub exclusion_source: String,
    pub exclusion_comment: Option<String>,
}

/// Configuration for the CIDR optimizer.
pub struct OptimizerConfig {
    pub ipv4_target: Option<TargetSpec>,
    pub ipv6_target: Option<TargetSpec>,
    pub max_over_coverage_ratio: Option<f64>,
    /// Maximum prefix length for IPv4 inputs — e.g., 24 means all prefixes more specific than /24 are truncated to /24 before aggregation.
    pub max_prefix_len_v4: u8,
    /// Maximum prefix length for IPv6 inputs — e.g., 48 means all prefixes more specific than /48 are truncated to /48 before aggregation.
    pub max_prefix_len_v6: u8,
    /// Pre-allocation guard: reject inputs exceeding this count before building the prefix tree.
    pub max_input_entries: usize,
    pub source_map: bool,
    pub exclusions: Vec<ExclusionEntry>,
    pub preferred_cidrs: Vec<PreferredEntry>,
    /// Constrains widening into non-preferred space independently of the global over-coverage cap.
    pub max_non_preferred_over_coverage_ratio: Option<f64>,
}

impl Default for OptimizerConfig {
    fn default() -> Self {
        Self {
            ipv4_target: None,
            ipv6_target: None,
            max_over_coverage_ratio: None,
            max_prefix_len_v4: 32,
            max_prefix_len_v6: 128,
            max_input_entries: 10_000_000,
            source_map: false,
            exclusions: Vec::new(),
            preferred_cidrs: Vec::new(),
            max_non_preferred_over_coverage_ratio: None,
        }
    }
}

/// Discriminates between IPv4 and IPv6 address families for per-family optimization.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddressFamily {
    IPv4,
    IPv6,
}

/// Progress information passed to callback.
pub enum Phase {
    Parsing { entries_read: usize },
    Lossless { af: AddressFamily, entries_remaining: usize },
    Lossy { af: AddressFamily, current_count: usize, target: usize },
    Done,
}

/// A single output prefix produced by optimization, with optional source-map and over-coverage tracking.
pub struct AggregatedEntry {
    pub prefix: IpNet,
    /// Original input indices that this output prefix covers (populated when source-map is enabled).
    pub source_indices: Option<Vec<usize>>,
    /// Number of extra IP addresses beyond the original input coverage (0 for lossless entries).
    pub over_coverage: u128,
    /// Exclusion zones that intersect this output prefix (populated when exclusions are configured).
    pub exclusion_collisions: Option<Vec<ExclusionCollision>>,
    /// Portion of over-coverage that falls within preferred CIDR ranges (discounted from cost).
    pub preferred_over_coverage: u128,
    /// Preferred entries that contributed to this entry's over-coverage (for source-map audit).
    pub preferred_contributions: Option<Vec<PreferredContribution>>,
}

/// Complete result of an optimization run: output entries and aggregate statistics.
pub struct OptimizationResult {
    pub entries: Vec<AggregatedEntry>,
    pub stats: OptimizationStats,
}

/// Aggregate statistics from an optimization run: counts, over-coverage totals, and constraint flags.
pub struct OptimizationStats {
    pub input_ipv4_count: usize,
    pub input_ipv6_count: usize,
    pub output_ipv4_count: usize,
    pub output_ipv6_count: usize,
    /// Total extra IPv4 addresses introduced by widening (number of IPs, not entries).
    pub total_ipv4_over_coverage: u128,
    /// Total extra IPv6 addresses introduced by widening (number of IPs, not entries).
    pub total_ipv6_over_coverage: u128,
    pub ipv4_compression_ratio: f64,
    pub ipv6_compression_ratio: f64,
    /// True when the IPv4 target was the binding constraint (lossless result exceeded target).
    pub ipv4_target_binding: bool,
    /// True when the IPv6 target was the binding constraint (lossless result exceeded target).
    pub ipv6_target_binding: bool,
    /// True when exclusion zones prevented reaching the IPv4 entry target.
    pub ipv4_exclusion_constrained: bool,
    /// True when exclusion zones prevented reaching the IPv6 entry target.
    pub ipv6_exclusion_constrained: bool,
    /// Over-coverage IPs that fall within preferred CIDR ranges (IPv4).
    pub total_ipv4_preferred_over_coverage: u128,
    /// Over-coverage IPs that fall within preferred CIDR ranges (IPv6).
    pub total_ipv6_preferred_over_coverage: u128,
    /// Over-coverage IPs outside preferred ranges (IPv4): total - preferred.
    pub total_ipv4_non_preferred_over_coverage: u128,
    /// Over-coverage IPs outside preferred ranges (IPv6): total - preferred.
    pub total_ipv6_non_preferred_over_coverage: u128,
    /// Total IP addresses covered by the original IPv4 input set (before optimization).
    pub input_ipv4_covered_ips: u128,
    /// Total IP addresses covered by the original IPv6 input set (before optimization).
    pub input_ipv6_covered_ips: u128,
}

/// A single input entry with its original text and optional inline comment.
#[derive(Clone, Debug, PartialEq)]
pub struct InputEntry {
    pub original: String,
    pub comment: Option<String>,
}

/// Result from `optimize_from_reader`: optimization output plus input metadata.
pub struct ReaderResult {
    pub result: OptimizationResult,
    pub input_metadata: Vec<InputEntry>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_values() {
        let cfg = OptimizerConfig::default();
        assert_eq!(cfg.ipv4_target, None::<TargetSpec>);
        assert_eq!(cfg.ipv6_target, None::<TargetSpec>);
        assert_eq!(cfg.max_over_coverage_ratio, None);
        assert_eq!(cfg.max_prefix_len_v4, 32);
        assert_eq!(cfg.max_prefix_len_v6, 128);
        assert_eq!(cfg.max_input_entries, 10_000_000);
        assert!(!cfg.source_map);
    }

    #[test]
    fn target_spec_from_str_integer() {
        let ts: TargetSpec = "60".parse().unwrap();
        assert_eq!(ts, TargetSpec::EntryCount(60));
    }

    #[test]
    fn target_spec_from_str_zero() {
        let ts: TargetSpec = "0".parse().unwrap();
        assert_eq!(ts, TargetSpec::EntryCount(0));
    }

    #[test]
    fn target_spec_from_str_over_coverage() {
        let ts: TargetSpec = "over-coverage=0.1%".parse().unwrap();
        assert_eq!(ts, TargetSpec::MaxOverCoverage(0.001));
    }

    #[test]
    fn target_spec_from_str_over_coverage_zero() {
        let ts: TargetSpec = "over-coverage=0%".parse().unwrap();
        assert_eq!(ts, TargetSpec::MaxOverCoverage(0.0));
    }

    #[test]
    fn target_spec_from_str_missing_percent() {
        let result = "over-coverage=0.1".parse::<TargetSpec>();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("must end with '%'"));
    }

    #[test]
    fn target_spec_from_str_invalid_number() {
        let result = "over-coverage=abc%".parse::<TargetSpec>();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("invalid over-coverage percentage"));
    }

    #[test]
    fn target_spec_from_str_negative() {
        let result = "over-coverage=-1%".parse::<TargetSpec>();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("non-negative"));
    }

    #[test]
    fn target_spec_from_str_garbage() {
        let result = "not_a_number".parse::<TargetSpec>();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("invalid target"));
    }

    #[test]
    fn target_spec_from_str_empty() {
        let result = "".parse::<TargetSpec>();
        assert!(result.is_err());
    }
}
