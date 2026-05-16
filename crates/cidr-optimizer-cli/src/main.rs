use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process;

use anyhow::Result;
use clap::{Parser, ValueEnum};
use serde::Serialize;

use cidr_optimizer::{optimize_from_reader, ExclusionEntry, OptimizeError, OptimizerConfig, OptimizerError, TargetSpec, parse_exclusions};

#[derive(Parser)]
#[command(name = "cidr-optimizer")]
#[command(about = "Optimize IP/CIDR lists to fit per-AF entry budgets")]
struct Cli {
    /// Input file (- for stdin)
    #[arg(default_value = "-")]
    input: String,

    /// IPv4 target: entry count (e.g. "60") or over-coverage ratio (e.g. "over-coverage=0.1%")
    #[arg(long)]
    ipv4_target: Option<String>,

    /// IPv6 target: entry count (e.g. "60") or over-coverage ratio (e.g. "over-coverage=0.1%")
    #[arg(long)]
    ipv6_target: Option<String>,

    /// Maximum over-coverage percentage per AF (0-1000%, or -1 to disable)
    #[arg(long, allow_negative_numbers = true)]
    max_over_coverage: Option<f64>,

    /// Maximum output prefix length for IPv4
    #[arg(long, default_value = "32")]
    max_prefix_len_v4: u8,

    /// Maximum output prefix length for IPv6
    #[arg(long, default_value = "128")]
    max_prefix_len_v6: u8,

    /// Maximum input entries
    #[arg(long, default_value = "10000000")]
    max_input_entries: usize,

    /// Output format
    #[arg(long, value_enum, default_value = "plain")]
    format: OutputFormat,

    /// Write source-map JSON to FILE
    #[arg(long, value_name = "FILE")]
    source_map: Option<PathBuf>,

    /// Show statistics on stderr
    #[arg(long)]
    stats: bool,

    /// Exclusion CIDR file (can be specified multiple times)
    #[arg(long = "exclude-cidr", value_name = "FILE")]
    exclude_cidrs: Vec<PathBuf>,

    /// Warn on stderr when input CIDRs overlap exclusion zones
    #[arg(long)]
    warn_on_excluded_input: bool,
}

#[derive(Clone, ValueEnum)]
enum OutputFormat {
    Plain,
    Json,
    Aws,
}

// Serde types for JSON output
#[derive(Serialize)]
struct JsonOutput {
    ipv4: Vec<JsonEntry>,
    ipv6: Vec<JsonEntry>,
    stats: JsonStats,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    exclusion_sources: Vec<ExclusionSourceInfo>,
}

#[derive(Serialize)]
struct JsonEntry {
    prefix: String,
    source_count: usize,
    over_coverage: u128,
}

#[derive(Clone, Serialize)]
struct ExclusionSourceInfo {
    source: String,
    entry_count: usize,
}

#[derive(Serialize)]
struct JsonStats {
    input_ipv4_count: usize,
    input_ipv6_count: usize,
    output_ipv4_count: usize,
    output_ipv6_count: usize,
    total_ipv4_over_coverage: u128,
    total_ipv6_over_coverage: u128,
    ipv4_compression_ratio: f64,
    ipv6_compression_ratio: f64,
    ipv4_target_binding: bool,
    ipv6_target_binding: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    ipv4_exclusion_constrained: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    ipv6_exclusion_constrained: bool,
}

#[derive(Serialize)]
struct AwsEntry {
    #[serde(rename = "Cidr")]
    cidr: String,
}

#[derive(Serialize)]
struct SourceMapOutput {
    entries: Vec<SourceMapEntry>,
    stats: JsonStats,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    exclusion_sources: Vec<ExclusionSourceInfo>,
}

#[derive(Serialize)]
struct SourceMapEntry {
    prefix: String,
    sources: Vec<SourceMapSource>,
    over_coverage: u128,
    #[serde(skip_serializing_if = "Option::is_none")]
    exclusion_collisions: Option<Vec<ExclusionCollisionJson>>,
}

#[derive(Serialize)]
struct SourceMapSource {
    index: usize,
    cidr: String,
    comment: Option<String>,
}

#[derive(Serialize)]
struct ExclusionCollisionJson {
    exclusion_prefix: String,
    exclusion_source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    exclusion_comment: Option<String>,
}

fn parse_target_spec(s: &str) -> Result<TargetSpec> {
    s.parse::<TargetSpec>().map_err(|e| anyhow::anyhow!("{}", e))
}

/// Parse exclusion files and return (entries, per-file source info).
fn parse_exclusion_files(paths: &[PathBuf]) -> Result<(Vec<ExclusionEntry>, Vec<ExclusionSourceInfo>)> {
    let mut entries = Vec::new();
    let mut source_info = Vec::new();

    for path in paths {
        let file = File::open(path)
            .map_err(|e| anyhow::anyhow!("cannot open exclusion file '{}': {}", path.display(), e))?;
        let filename = path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());

        let file_entries = parse_exclusions(BufReader::new(file), &filename)
            .map_err(|e| anyhow::anyhow!("{}: {}", path.display(), e))?;

        source_info.push(ExclusionSourceInfo {
            source: filename,
            entry_count: file_entries.len(),
        });
        entries.extend(file_entries);
    }

    Ok((entries, source_info))
}

fn build_json_entry(
    e: &cidr_optimizer::AggregatedEntry,
) -> JsonEntry {
    JsonEntry {
        prefix: e.prefix.to_string(),
        source_count: e.source_indices.as_ref().map_or(0, |s| s.len()),
        over_coverage: e.over_coverage,
    }
}

fn build_json_stats(stats: &cidr_optimizer::OptimizationStats) -> JsonStats {
    JsonStats {
        input_ipv4_count: stats.input_ipv4_count,
        input_ipv6_count: stats.input_ipv6_count,
        output_ipv4_count: stats.output_ipv4_count,
        output_ipv6_count: stats.output_ipv6_count,
        total_ipv4_over_coverage: stats.total_ipv4_over_coverage,
        total_ipv6_over_coverage: stats.total_ipv6_over_coverage,
        ipv4_compression_ratio: stats.ipv4_compression_ratio,
        ipv6_compression_ratio: stats.ipv6_compression_ratio,
        ipv4_target_binding: stats.ipv4_target_binding,
        ipv6_target_binding: stats.ipv6_target_binding,
        ipv4_exclusion_constrained: stats.ipv4_exclusion_constrained,
        ipv6_exclusion_constrained: stats.ipv6_exclusion_constrained,
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let ipv4_target = cli.ipv4_target.as_deref().map(parse_target_spec).transpose()?;
    let ipv6_target = cli.ipv6_target.as_deref().map(parse_target_spec).transpose()?;

    // Validate conflict: MaxOverCoverage target + --max-over-coverage
    let has_over_coverage_target = matches!(ipv4_target, Some(TargetSpec::MaxOverCoverage(_)))
        || matches!(ipv6_target, Some(TargetSpec::MaxOverCoverage(_)));
    if has_over_coverage_target && cli.max_over_coverage.is_some() {
        eprintln!("error: --max-over-coverage cannot be used with over-coverage=X% target syntax");
        process::exit(1);
    }

    // Convert percentage to ratio. -1 disables capping entirely.
    // Default to 100% when an EntryCount target is set without explicit value.
    let has_entry_count_target = matches!(ipv4_target, Some(TargetSpec::EntryCount(_)))
        || matches!(ipv6_target, Some(TargetSpec::EntryCount(_)));
    let effective_ratio = match cli.max_over_coverage {
        Some(-1.0) => None,
        Some(pct) => Some(pct / 100.0),
        None if has_entry_count_target => Some(1.0),
        None => None,
    };

    // Parse exclusion files
    let (exclusions, exclusion_sources) = parse_exclusion_files(&cli.exclude_cidrs)?;

    let config = OptimizerConfig {
        ipv4_target,
        ipv6_target,
        max_over_coverage_ratio: effective_ratio,
        max_prefix_len_v4: cli.max_prefix_len_v4,
        max_prefix_len_v6: cli.max_prefix_len_v6,
        max_input_entries: cli.max_input_entries,
        source_map: cli.source_map.is_some(),
        exclusions,
    };

    // Run optimization
    let reader: Box<dyn BufRead> = if cli.input == "-" {
        Box::new(BufReader::new(io::stdin()))
    } else {
        Box::new(BufReader::new(File::open(&cli.input)?))
    };
    let (result, input_metadata) = match optimize_from_reader(reader, &config) {
        Ok(reader_result) => (reader_result.result, reader_result.input_metadata),
        Err(OptimizerError::Optimize(OptimizeError::CoverageLost)) => {
            eprintln!("internal error: coverage invariant violated — this is a bug, please report it");
            process::exit(1);
        }
        Err(e) => return Err(e.into()),
    };

    // Exit code 2: exclusion-constrained (more specific than ratio-cap)
    if result.stats.ipv4_exclusion_constrained || result.stats.ipv6_exclusion_constrained {
        if result.stats.ipv4_exclusion_constrained {
            eprintln!(
                "error: IPv4 target unreachable — exclusion zones prevent sufficient merging\n\
                 hint: reduce exclusion entries or raise the target"
            );
        }
        if result.stats.ipv6_exclusion_constrained {
            eprintln!(
                "error: IPv6 target unreachable — exclusion zones prevent sufficient merging\n\
                 hint: reduce exclusion entries or raise the target"
            );
        }
        process::exit(2);
    }

    // Fail hard if EntryCount target was not met (not for MaxOverCoverage)
    if let Some(TargetSpec::EntryCount(t)) = ipv4_target {
        if result.stats.output_ipv4_count > t {
            eprintln!(
                "error: IPv4 target {} unreachable — got {} entries (over-coverage cap prevents further merging)\n\
                 hint: raise the cap with --max-over-coverage <percentage> (up to 1000) or disable it with --max-over-coverage -1",
                t, result.stats.output_ipv4_count
            );
            process::exit(2);
        }
    }
    if let Some(TargetSpec::EntryCount(t)) = ipv6_target {
        if result.stats.output_ipv6_count > t {
            eprintln!(
                "error: IPv6 target {} unreachable — got {} entries (over-coverage cap prevents further merging)\n\
                 hint: raise the cap with --max-over-coverage <percentage> (up to 1000) or disable it with --max-over-coverage -1",
                t, result.stats.output_ipv6_count
            );
            process::exit(2);
        }
    }

    // Warn on stderr if input overlaps exclusion zones
    if cli.warn_on_excluded_input {
        for entry in &result.entries {
            if let Some(ref collisions) = entry.exclusion_collisions {
                for c in collisions {
                    eprintln!(
                        "warning: output prefix {} overlaps exclusion {} from {}",
                        entry.prefix, c.exclusion_prefix, c.exclusion_source
                    );
                }
            }
        }
    }

    // Warn on stderr if over-coverage exceeds 1000%
    if effective_ratio.is_none() {
        let input_v4_ips = result.stats.input_ipv4_count as u128;
        let input_v6_ips = result.stats.input_ipv6_count as u128;
        if (input_v4_ips > 0 && result.stats.total_ipv4_over_coverage > input_v4_ips * 10)
            || (input_v6_ips > 0 && result.stats.total_ipv6_over_coverage > input_v6_ips * 10)
        {
            eprintln!("warning: over-coverage exceeds 1000%");
        }
    }

    if cli.stats {
        eprintln!(
            "IPv4: {} input → {} output (compression: {:.1}x)",
            result.stats.input_ipv4_count,
            result.stats.output_ipv4_count,
            result.stats.ipv4_compression_ratio,
        );
        eprintln!(
            "IPv6: {} input → {} output (compression: {:.1}x)",
            result.stats.input_ipv6_count,
            result.stats.output_ipv6_count,
            result.stats.ipv6_compression_ratio,
        );
        if result.stats.total_ipv4_over_coverage > 0 {
            eprintln!("IPv4 over-coverage: {} IPs", result.stats.total_ipv4_over_coverage);
        }
        if result.stats.total_ipv6_over_coverage > 0 {
            eprintln!("IPv6 over-coverage: {} IPs", result.stats.total_ipv6_over_coverage);
        }
    }

    match cli.format {
        OutputFormat::Plain => {
            for entry in &result.entries {
                println!("{}", entry.prefix);
            }
        }
        OutputFormat::Json => {
            let json_output = JsonOutput {
                ipv4: result.entries.iter()
                    .filter(|e| matches!(e.prefix, ipnet::IpNet::V4(_)))
                    .map(build_json_entry)
                    .collect(),
                ipv6: result.entries.iter()
                    .filter(|e| matches!(e.prefix, ipnet::IpNet::V6(_)))
                    .map(build_json_entry)
                    .collect(),
                stats: build_json_stats(&result.stats),
                exclusion_sources: exclusion_sources.clone(),
            };
            println!("{}", serde_json::to_string_pretty(&json_output)?);
        }
        OutputFormat::Aws => {
            let aws_entries: Vec<AwsEntry> = result.entries.iter()
                .map(|e| AwsEntry { cidr: e.prefix.to_string() })
                .collect();
            println!("{}", serde_json::to_string_pretty(&aws_entries)?);
        }
    }

    // Write source-map file if requested
    if let Some(ref path) = cli.source_map {
        let sm_entries: Vec<SourceMapEntry> = result.entries.iter().map(|e| {
            let sources = e.source_indices.as_ref().map_or_else(Vec::new, |indices| {
                indices.iter().map(|&i| {
                    if i < input_metadata.len() {
                        let meta = &input_metadata[i];
                        SourceMapSource { index: i, cidr: meta.original.clone(), comment: meta.comment.clone() }
                    } else {
                        SourceMapSource { index: i, cidr: format!("index:{}", i), comment: None }
                    }
                }).collect()
            });
            let exclusion_collisions = e.exclusion_collisions.as_ref().map(|collisions| {
                collisions.iter().map(|c| ExclusionCollisionJson {
                    exclusion_prefix: c.exclusion_prefix.clone(),
                    exclusion_source: c.exclusion_source.clone(),
                    exclusion_comment: c.exclusion_comment.clone(),
                }).collect()
            });
            SourceMapEntry {
                prefix: e.prefix.to_string(),
                sources,
                over_coverage: e.over_coverage,
                exclusion_collisions,
            }
        }).collect();

        let sm_output = SourceMapOutput {
            entries: sm_entries,
            stats: build_json_stats(&result.stats),
            exclusion_sources: exclusion_sources.clone(),
        };

        let mut file = File::create(path)
            .map_err(|e| anyhow::anyhow!("cannot create source-map file '{}': {}", path.display(), e))?;
        let json = serde_json::to_string_pretty(&sm_output)?;
        file.write_all(json.as_bytes())?;
        file.write_all(b"\n")?;
    }

    Ok(())
}
