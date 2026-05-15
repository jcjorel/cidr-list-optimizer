# User Guide

## CLI Reference

```
cidr-optimizer [OPTIONS] [INPUT]
```

### Arguments

| Argument | Description | Default |
|----------|-------------|---------|
| `INPUT` | Input file path, or `-` for stdin | `-` |

### Options

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `--ipv4-target <SPEC>` | String | None (lossless) | IPv4 target: integer entry count (e.g. `60`) or `over-coverage=X%` (e.g. `over-coverage=0.1%`) |
| `--ipv6-target <SPEC>` | String | None (lossless) | IPv6 target: integer entry count (e.g. `60`) or `over-coverage=X%` (e.g. `over-coverage=0.1%`) |
| `--max-over-coverage <PCT>` | f64 | 100 (when target set) | Maximum over-coverage percentage per AF. Use `-1` to disable. **Cannot** be combined with `over-coverage=X%` target syntax |
| `--max-prefix-len-v4 <N>` | u8 | 32 | Maximum output prefix length for IPv4 |
| `--max-prefix-len-v6 <N>` | u8 | 128 | Maximum output prefix length for IPv6 |
| `--max-input-entries <N>` | usize | 10,000,000 | Maximum input entries before error |
| `--format <FMT>` | enum | `plain` | Output format: `plain`, `json`, `aws` |
| `--source-map <FILE>` | PathBuf | — | Write source-map JSON to FILE |
| `--stats` | bool | false | Print statistics to stderr |
| `--validate` | bool | false | Verify all inputs are covered by output |

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Validation failure (`--validate` and coverage lost) |
| 2 | Target unreachable (over-coverage cap prevents meeting budget) |

### `--max-over-coverage` Behavior

The value is a **percentage** (not a ratio):
- `--max-over-coverage 5` → cap at 5% over-coverage per AF
- `--max-over-coverage 100` → cap at 100% (default when any entry-count target is set)
- `--max-over-coverage 1000` → cap at 1000% (maximum allowed value)
- `--max-over-coverage -1` → disable cap entirely (merge as aggressively as needed)
- Omitted with no target → no cap (lossless mode has zero over-coverage)

When the cap is reached before the target, merging stops early. If the resulting entry count still exceeds the target, the CLI exits with code 2 and a hint to raise the cap.

**Conflict rule**: `--max-over-coverage` cannot be used together with `over-coverage=X%` target syntax (they express the same constraint differently). If both are specified, the CLI exits with an error:

```
error: --max-over-coverage cannot be used with over-coverage=X% target syntax
```

## Library API

### Primary Functions

```rust
use cidr_optimizer::{optimize, optimize_with_progress, optimize_from_reader};
use cidr_optimizer::{OptimizerConfig, OptimizationResult, Phase};
use ipnet::IpNet;
use std::ops::ControlFlow;

// From pre-parsed prefixes
let result = optimize(&prefixes, &config)?;

// With progress/cancellation
let result = optimize_with_progress(&prefixes, &config, |phase| {
    match phase {
        Phase::Lossy { current_count, target, .. } => {
            println!("Reducing: {} → {}", current_count, target);
        }
        Phase::Done => println!("Complete"),
        _ => {}
    }
    ControlFlow::Continue(())  // Return Break(()) to cancel
})?;

// From a reader (parses internally)
let result = optimize_from_reader(reader, &config)?;
```

### `TargetSpec` Enum

Specifies how the optimization target is defined per address family:

```rust
pub enum TargetSpec {
    /// Fixed entry count target — reduce to at most N entries.
    EntryCount(usize),
    /// Find minimum entries keeping over-coverage ≤ ratio (e.g., 0.001 for 0.1%).
    MaxOverCoverage(f64),
}
```

The CLI parses `--ipv4-target` / `--ipv6-target` strings into `TargetSpec`:
- Integer (e.g. `"60"`) → `TargetSpec::EntryCount(60)`
- `"over-coverage=X%"` (e.g. `"over-coverage=0.1%"`) → `TargetSpec::MaxOverCoverage(0.001)`

### `OptimizerConfig` Fields

| Field | Type | Default | Valid Range | Effect |
|-------|------|---------|-------------|--------|
| `ipv4_target` | `Option<TargetSpec>` | `None` | — | IPv4 optimization target. `None` = lossless |
| `ipv6_target` | `Option<TargetSpec>` | `None` | — | IPv6 optimization target. `None` = lossless |
| `max_over_coverage_ratio` | `Option<f64>` | `None` | 0.0..=10.0 | Ratio cap (1.0 = 100%). `None` = no cap |
| `max_prefix_len_v4` | `u8` | `32` | 1..=32 | Longest IPv4 prefix in output |
| `max_prefix_len_v6` | `u8` | `128` | 1..=128 | Longest IPv6 prefix in output |
| `max_input_entries` | `usize` | `10_000_000` | 1..=∞ | Input size bound |
| `source_map` | `bool` | `false` | — | Track which inputs map to each output |

### `Phase` Enum (Progress Callbacks)

```rust
pub enum Phase {
    Parsing { entries_read: usize },
    Lossless { af: AddressFamily, entries_remaining: usize },
    Lossy { af: AddressFamily, current_count: usize, target: usize },
    Done,
}

pub enum AddressFamily { IPv4, IPv6 }
```

Progress callbacks receive `Phase` values at each stage transition. Return `ControlFlow::Break(())` from the callback to cancel optimization (returns `OptimizeError::Cancelled`).

### `OptimizationResult`

```rust
pub struct OptimizationResult {
    pub entries: Vec<AggregatedEntry>,
    pub stats: OptimizationStats,
}

pub struct AggregatedEntry {
    pub prefix: IpNet,
    pub source_indices: Option<Vec<usize>>,  // None if source_map disabled
    pub over_coverage: u128,
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
    pub ipv4_target_binding: bool,   // true if lossless exceeded target
    pub ipv6_target_binding: bool,
}
```

## Error Types

### `OptimizeError` (from `optimize` / `optimize_with_progress`)

| Variant | Meaning |
|---------|---------|
| `EmptyInput` | No valid prefixes in input |
| `InvalidConfig { message }` | Configuration parameter out of valid range |
| `TargetTooSmall { target, minimum }` | Target is 0 but AF has entries |
| `InputTooLarge { count, limit }` | Input exceeds `max_input_entries` |
| `ArenaOverflow` | Trie exceeds u32::MAX nodes |
| `Cancelled` | Progress callback returned `Break(())` |
| `CoverageLost` | Internal invariant violation (bug) |

### `OptimizerError` (from `optimize_from_reader`)

| Variant | Meaning |
|---------|---------|
| `Optimize(OptimizeError)` | Wraps any `OptimizeError` |
| `Parse { line, message }` | Invalid input at specific line |
| `Io(std::io::Error)` | I/O failure reading input |

### Error Handling Pattern

```rust
use cidr_optimizer::{optimize_from_reader, OptimizerError, OptimizeError};

match optimize_from_reader(reader, &config) {
    Ok(result) => { /* use result */ }
    Err(OptimizerError::Optimize(OptimizeError::Cancelled)) => {
        eprintln!("Optimization cancelled by user");
    }
    Err(OptimizerError::Optimize(OptimizeError::InputTooLarge { count, limit })) => {
        eprintln!("Input has {} entries, limit is {}", count, limit);
    }
    Err(OptimizerError::Parse { line, message }) => {
        eprintln!("Parse error at line {}: {}", line, message);
    }
    Err(e) => eprintln!("Error: {}", e),
}
```

## Output Formats

### Plain (default)

One CIDR per line, sorted by network address ascending (IPv4 before IPv6):

```
10.0.0.0/22
192.168.0.0/16
2001:db8::/32
```

### JSON

```json
{
  "ipv4": [
    {
      "prefix": "10.0.0.0/22",
      "source_count": 4,
      "over_coverage": 384
    }
  ],
  "ipv6": [],
  "stats": {
    "input_ipv4_count": 500000,
    "input_ipv6_count": 0,
    "output_ipv4_count": 1000,
    "output_ipv6_count": 0,
    "total_ipv4_over_coverage": 28456,
    "total_ipv6_over_coverage": 0,
    "ipv4_compression_ratio": 500.0,
    "ipv6_compression_ratio": 1.0,
    "ipv4_target_binding": true,
    "ipv6_target_binding": false
  }
}
```

Fields:
- `prefix`: Output CIDR string
- `source_count`: Number of original inputs covered (0 if source-map disabled)
- `over_coverage`: Number of IPs in this prefix NOT in any input entry

### AWS

Array suitable for `aws ec2 modify-managed-prefix-list --add-entries`:

```json
[
  {"Cidr": "10.0.0.0/22"},
  {"Cidr": "192.168.0.0/16"},
  {"Cidr": "2001:db8::/32"}
]
```

## Source-Map File Format

When `--source-map <FILE>` is specified, the detailed source mapping is written to the given file in JSON format (regardless of `--format`):

```json
{
  "entries": [
    {
      "prefix": "10.0.0.0/22",
      "sources": [
        {"index": 0, "cidr": "10.0.0.0/24", "comment": null},
        {"index": 1, "cidr": "10.0.1.0/24", "comment": "partner-A"}
      ],
      "over_coverage": 512
    }
  ],
  "stats": {
    "input_ipv4_count": 500000,
    "input_ipv6_count": 0,
    "output_ipv4_count": 1000,
    "output_ipv6_count": 0,
    "total_ipv4_over_coverage": 28456,
    "total_ipv6_over_coverage": 0,
    "ipv4_compression_ratio": 500.0,
    "ipv6_compression_ratio": 1.0,
    "ipv4_target_binding": true,
    "ipv6_target_binding": false
  }
}
```

## Source-Map Interpretation

When `--source-map <FILE>` is used, the source-map file reports which original inputs each output prefix encompasses.

### Fields

| Field | Type | Description |
|-------|------|-------------|
| `source_indices` | `Vec<usize>` | Zero-based indices into the original input (line order, skipping comments/blanks) |
| `source_count` | `usize` | Length of `source_indices` |
| `over_coverage` | `u128` | IPs in this prefix not covered by any input |

### Interpretation

- `source_indices = [0, 1, 5]` means input entries at positions 0, 1, and 5 are covered by this output prefix
- `over_coverage = 0` means the output prefix exactly covers its inputs (lossless)
- `over_coverage > 0` means the prefix was widened during lossy optimization, including IPs not in the original input

### Production Usage

- **Audit trail**: Map firewall rules back to original allow-list entries
- **Incremental updates**: When an input entry is removed, identify which output CIDRs are affected
- **Cost analysis**: Rank output prefixes by over-coverage to understand aggregation cost

## `--validate` Behavior

When enabled, the CLI verifies that every input prefix is contained by at least one output prefix after optimization. This is a safety check — the algorithm maintains this invariant internally, but `--validate` provides an independent verification.

- Adds O(N log M) post-processing time (binary search per input against sorted output)
- Exits with code 1 if validation fails (indicates a bug)
- Prints "Validation passed: all inputs covered" to stderr on success

## `--stats` Output

Printed to stderr (does not interfere with stdout output):

```
IPv4: 500000 input → 1000 output (compression: 500.0x)
IPv6: 0 input → 0 output (compression: 1.0x)
IPv4 over-coverage: 28456 IPs
```

## Input Format

One CIDR prefix per line. Supports both IPv4 and IPv6:

```
10.0.0.0/24
192.168.1.0/25
2001:db8::/32
192.168.1.1
```

Rules:
- Lines starting with `#` are comments (skipped)
- Blank lines are skipped
- Bare IP addresses without a prefix length are treated as host addresses (`/32` for IPv4, `/128` for IPv6)
- Non-canonical CIDRs (host bits set) are normalized with a warning: `10.0.0.5/24` → `10.0.0.0/24`
- Maximum line length: 4096 bytes
- Entry indices for source-map are assigned sequentially, counting only valid entries (not comments/blanks)

## Additional Library API

### `validate_coverage`

```rust
pub fn validate_coverage(input: &[IpNet], output: &[AggregatedEntry]) -> bool
```

Verifies that every input prefix is contained by at least one output prefix. Uses binary search on sorted output (O(N log M)). Returns `true` if all inputs are covered.

### `parser::parse_input`

```rust
pub fn parse_input(
    input: impl BufRead,
    store_strings: bool,
    max_entries: usize,
) -> Result<ParsedInput, OptimizerError>
```

Parses input into partitioned IPv4/IPv6 vectors with original indices. Set `store_strings = true` to retain original input strings (needed for source-map display). Returns `ParsedInput` with fields:

| Field | Type | Description |
|-------|------|-------------|
| `ipv4` | `Vec<(usize, Ipv4Net)>` | Parsed IPv4 prefixes with original indices |
| `ipv6` | `Vec<(usize, Ipv6Net)>` | Parsed IPv6 prefixes with original indices |
| `original_strings` | `Vec<String>` | Original input lines (if `store_strings = true`) |
| `total_entries` | `usize` | Total valid entries parsed |
| `parse_warnings` | `Vec<(usize, String)>` | Non-fatal warnings (e.g., host bit truncation) |

### `OptimizationStats` Fields

| Field | Type | Description |
|-------|------|-------------|
| `input_ipv4_count` | `usize` | Number of IPv4 prefixes in input |
| `input_ipv6_count` | `usize` | Number of IPv6 prefixes in input |
| `output_ipv4_count` | `usize` | Number of IPv4 prefixes in output |
| `output_ipv6_count` | `usize` | Number of IPv6 prefixes in output |
| `total_ipv4_over_coverage` | `u128` | Total IPv4 IPs in output not in any input |
| `total_ipv6_over_coverage` | `u128` | Total IPv6 IPs in output not in any input |
| `ipv4_compression_ratio` | `f64` | `input_ipv4_count / output_ipv4_count` |
| `ipv6_compression_ratio` | `f64` | `input_ipv6_count / output_ipv6_count` |
| `ipv4_target_binding` | `bool` | `true` if lossless output exceeded IPv4 target (lossy phase ran) |
| `ipv6_target_binding` | `bool` | `true` if lossless output exceeded IPv6 target (lossy phase ran) |

## Integration Patterns

### As a Library Dependency

```toml
[dependencies]
cidr-optimizer = "1.0"
```

```rust
use cidr_optimizer::{optimize, OptimizerConfig, TargetSpec};
use ipnet::IpNet;

let prefixes: Vec<IpNet> = parse_your_feed();
let config = OptimizerConfig {
    ipv4_target: Some(TargetSpec::EntryCount(1000)),
    ..Default::default()
};
let result = optimize(&prefixes, &config)?;
for entry in &result.entries {
    apply_to_prefix_list(entry.prefix);
}
```

### As CLI in Scripts

```bash
#!/bin/bash
# Update prefix list from daily partner IP feed
curl -s https://feeds.example.com/partner-ips.txt \
  | cidr-optimizer --ipv4-target 1000 --format aws \
  | aws ec2 modify-managed-prefix-list \
      --prefix-list-id pl-0123456789abcdef0 \
      --current-version $(aws ec2 describe-managed-prefix-lists ...) \
      --add-entries file:///dev/stdin
```

### In CI/CD Pipelines

```bash
# Validate that optimization doesn't lose coverage
cidr-optimizer --ipv4-target 1000 --validate allow-list.txt > /dev/null
echo "Coverage check passed (exit $?)"
```

## Performance Tuning

### Choosing a Target

- Start with the AWS service limit (e.g., 1000 for prefix lists)
- If over-coverage is unacceptable, increase the target or lower `--max-over-coverage`
- Use `--stats` to see compression ratio and over-coverage before committing

### Over-Coverage Cap

- Default 100% when a target is set — allows doubling the covered IP space
- For security-sensitive use cases, use `--max-over-coverage 5` (5%) to limit collateral
- If the target is unreachable at your cap, the CLI exits with code 2 and suggests raising it

### `max_prefix_len` Tradeoffs

- Lowering `max_prefix_len_v4` (e.g., to 24) forces all /25–/32 inputs to widen to /24, increasing over-coverage but reducing entry count before the lossy phase even runs
- Useful when the downstream system has its own prefix length restrictions

### Memory vs Source-Map

- Source-map tracking increases memory usage proportionally to input size
- Disable source-map for maximum throughput when audit trail is not needed

### Large Inputs (>1M entries)

- Processing time scales linearly with input size
- If memory is constrained, reduce `max_input_entries` to fail fast rather than OOM
