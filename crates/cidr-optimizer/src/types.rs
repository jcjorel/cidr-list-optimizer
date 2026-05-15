use ipnet::IpNet;

/// Configuration for the CIDR optimizer.
pub struct OptimizerConfig {
    pub ipv4_target: Option<usize>,
    pub ipv6_target: Option<usize>,
    pub max_over_coverage_ratio: Option<f64>,
    pub max_prefix_len_v4: u8,
    pub max_prefix_len_v6: u8,
    pub max_input_entries: usize,
    pub provenance: bool,
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
            provenance: false,
        }
    }
}

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

/// A single output prefix with optional provenance.
pub struct AggregatedEntry {
    pub prefix: IpNet,
    pub source_indices: Option<Vec<usize>>,
    pub over_coverage: u128,
}

pub struct OptimizationResult {
    pub entries: Vec<AggregatedEntry>,
    pub stats: OptimizationStats,
}

pub struct OptimizationStats {
    pub input_ipv4_count: usize,
    pub input_ipv6_count: usize,
    pub output_ipv4_count: usize,
    pub output_ipv6_count: usize,
    pub total_ipv4_over_coverage: u128,
    pub total_ipv6_over_coverage: u128,
    pub ipv4_compression_ratio: f64,
    pub ipv6_compression_ratio: f64,
    pub ipv4_target_binding: bool,
    pub ipv6_target_binding: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_values() {
        let cfg = OptimizerConfig::default();
        assert_eq!(cfg.ipv4_target, None);
        assert_eq!(cfg.ipv6_target, None);
        assert_eq!(cfg.max_over_coverage_ratio, None);
        assert_eq!(cfg.max_prefix_len_v4, 32);
        assert_eq!(cfg.max_prefix_len_v6, 128);
        assert_eq!(cfg.max_input_entries, 10_000_000);
        assert!(!cfg.provenance);
    }
}
