use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::process;

use anyhow::Result;
use clap::{Parser, ValueEnum};
use serde::Serialize;

use cidr_optimizer::{optimize, optimize_from_reader, validate_coverage, OptimizerConfig};

#[derive(Parser)]
#[command(name = "cidr-optimizer")]
#[command(about = "Optimize IP/CIDR lists to fit per-AF entry budgets")]
struct Cli {
    /// Input file (- for stdin)
    #[arg(default_value = "-")]
    input: String,

    /// IPv4 target entry count (omit for lossless)
    #[arg(long)]
    ipv4_target: Option<usize>,

    /// IPv6 target entry count (omit for lossless)
    #[arg(long)]
    ipv6_target: Option<usize>,

    /// Maximum over-coverage ratio per AF
    #[arg(long)]
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

    /// Show provenance
    #[arg(long)]
    provenance: bool,

    /// Show statistics on stderr
    #[arg(long)]
    stats: bool,

    /// Validate output covers all inputs
    #[arg(long)]
    validate: bool,
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
}

#[derive(Serialize)]
struct JsonEntry {
    prefix: String,
    source_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    sources: Option<Vec<String>>,
    over_coverage: u128,
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
}

#[derive(Serialize)]
struct AwsEntry {
    #[serde(rename = "Cidr")]
    cidr: String,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let config = OptimizerConfig {
        ipv4_target: cli.ipv4_target,
        ipv6_target: cli.ipv6_target,
        max_over_coverage_ratio: cli.max_over_coverage,
        max_prefix_len_v4: cli.max_prefix_len_v4,
        max_prefix_len_v6: cli.max_prefix_len_v6,
        max_input_entries: cli.max_input_entries,
        provenance: cli.provenance,
    };

    // For --validate, we need the original parsed prefixes
    let result = if cli.validate {
        let reader: Box<dyn BufRead> = if cli.input == "-" {
            Box::new(BufReader::new(io::stdin()))
        } else {
            Box::new(BufReader::new(File::open(&cli.input)?))
        };

        // Parse input first to retain original prefixes for validation
        let parsed = cidr_optimizer::parser::parse_input(reader, config.provenance, config.max_input_entries)?;
        let prefixes: Vec<ipnet::IpNet> = parsed
            .ipv4.iter().map(|(_, p)| ipnet::IpNet::V4(*p))
            .chain(parsed.ipv6.iter().map(|(_, p)| ipnet::IpNet::V6(*p)))
            .collect();

        let opt_result = optimize(&prefixes, &config)?;

        // Validate coverage
        if !validate_coverage(&prefixes, &opt_result.entries) {
            eprintln!("error: validation failed — not all inputs are covered by output");
            process::exit(1);
        }
        eprintln!("Validation passed: all inputs covered");

        opt_result
    } else {
        let reader: Box<dyn BufRead> = if cli.input == "-" {
            Box::new(BufReader::new(io::stdin()))
        } else {
            Box::new(BufReader::new(File::open(&cli.input)?))
        };
        optimize_from_reader(reader, &config)?
    };

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
                    .map(|e| JsonEntry {
                        prefix: e.prefix.to_string(),
                        source_count: e.source_indices.as_ref().map_or(0, |s| s.len()),
                        sources: if cli.provenance {
                            e.source_indices.as_ref().map(|indices| {
                                indices.iter().map(|i| format!("index:{}", i)).collect()
                            })
                        } else {
                            None
                        },
                        over_coverage: e.over_coverage,
                    })
                    .collect(),
                ipv6: result.entries.iter()
                    .filter(|e| matches!(e.prefix, ipnet::IpNet::V6(_)))
                    .map(|e| JsonEntry {
                        prefix: e.prefix.to_string(),
                        source_count: e.source_indices.as_ref().map_or(0, |s| s.len()),
                        sources: if cli.provenance {
                            e.source_indices.as_ref().map(|indices| {
                                indices.iter().map(|i| format!("index:{}", i)).collect()
                            })
                        } else {
                            None
                        },
                        over_coverage: e.over_coverage,
                    })
                    .collect(),
                stats: JsonStats {
                    input_ipv4_count: result.stats.input_ipv4_count,
                    input_ipv6_count: result.stats.input_ipv6_count,
                    output_ipv4_count: result.stats.output_ipv4_count,
                    output_ipv6_count: result.stats.output_ipv6_count,
                    total_ipv4_over_coverage: result.stats.total_ipv4_over_coverage,
                    total_ipv6_over_coverage: result.stats.total_ipv6_over_coverage,
                    ipv4_compression_ratio: result.stats.ipv4_compression_ratio,
                    ipv6_compression_ratio: result.stats.ipv6_compression_ratio,
                    ipv4_target_binding: result.stats.ipv4_target_binding,
                    ipv6_target_binding: result.stats.ipv6_target_binding,
                },
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

    Ok(())
}
