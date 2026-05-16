# Developer API Reference

Library crate integration reference for `cidr-optimizer`.

## Installation

```toml
[dependencies]
cidr-optimizer = "1.1"
ipnet = "2"
```

**MSRV**: Rust 1.93+ (edition 2021)

## Re-Export Structure

The crate re-exports all public types from the root:

```rust
pub use types::{
    AddressFamily, AggregatedEntry, ExclusionCollision, ExclusionEntry,
    InputEntry, OptimizerConfig, OptimizationResult, OptimizationStats,
    Phase, ReaderResult, TargetSpec,
};
pub use error::{OptimizeError, OptimizerError};
```

### Module Visibility

| Module | Visibility | Notes |
|--------|-----------|-------|
| `types` | `pub` | All data types |
| `error` | `pub` | Error enums |
| `parser` | `pub` | `parser::parse_input` accessible but not re-exported at root |
| `lossless` | `pub` | Internal aggregation (not intended for direct use) |
| `trie` | `pub` | Internal trie structure (not intended for direct use) |
| `optimizer` | `pub` | Internal greedy optimizer (not intended for direct use) |
| `source_map` | `pub` | Internal source-map computation (not intended for direct use) |
| `exclusion` | `pub` | Exclusion set construction and intersection queries (not intended for direct use) |

## Primary Functions

### `optimize`

```rust
pub fn optimize(
    prefixes: &[IpNet],
    config: &OptimizerConfig,
) -> Result<OptimizationResult, OptimizeError>
```

Optimizes pre-parsed prefixes. This is the main entry point when you already have parsed `IpNet` values.

### `optimize_with_progress`

```rust
pub fn optimize_with_progress(
    prefixes: &[IpNet],
    config: &OptimizerConfig,
    progress: impl FnMut(Phase) -> ControlFlow<()>,
) -> Result<OptimizationResult, OptimizeError>
```

Same as `optimize` but accepts a progress callback. Return `ControlFlow::Break(())` from the callback to cancel (returns `OptimizeError::Cancelled`).

### `optimize_from_reader`

```rust
pub fn optimize_from_reader(
    input: impl BufRead,
    config: &OptimizerConfig,
) -> Result<ReaderResult, OptimizerError>
```

Parses input from a reader and optimizes in one step. Returns both the optimization result and input metadata (original strings and comments for source-map display).

### `validate_coverage`

```rust
pub fn validate_coverage(input: &[IpNet], output: &[AggregatedEntry]) -> bool
```

Verifies that every input prefix is contained by at least one output prefix. This is called internally by `optimize_with_progress` before returning results — you do not need to call it manually unless implementing custom pipelines. Returns `false` if coverage is lost (which would indicate a bug).

## `TargetSpec`

```rust
pub enum TargetSpec {
    /// Fixed entry count target — reduce to at most N entries.
    EntryCount(usize),
    /// Find minimum entries keeping over-coverage ≤ ratio (e.g., 0.001 for 0.1%).
    MaxOverCoverage(f64),
}
```

## `OptimizerConfig` Fields

| Field | Type | Default | Valid Range | Effect |
|-------|------|---------|-------------|--------|
| `ipv4_target` | `Option<TargetSpec>` | `None` | — | IPv4 optimization target. `None` = lossless only |
| `ipv6_target` | `Option<TargetSpec>` | `None` | — | IPv6 optimization target. `None` = lossless only |
| `max_over_coverage_ratio` | `Option<f64>` | `None` | 0.0..=10.0 | Ratio cap (1.0 = 100%). `None` = no cap. Cannot be set when using `MaxOverCoverage` target |
| `max_prefix_len_v4` | `u8` | `32` | 1..=32 | Longest IPv4 prefix length in output |
| `max_prefix_len_v6` | `u8` | `128` | 1..=128 | Longest IPv6 prefix length in output |
| `max_input_entries` | `usize` | `10_000_000` | 1..=∞ | Input size bound (prevents OOM) |
| `source_map` | `bool` | `false` | — | Track which inputs map to each output prefix |
| `exclusions` | `Vec<ExclusionEntry>` | `Vec::new()` | — | CIDR ranges protected from absorption during lossy optimization |

`OptimizerConfig` implements `Default`.

## `Phase` and `AddressFamily`

```rust
pub enum Phase {
    Parsing { entries_read: usize },
    Lossless { af: AddressFamily, entries_remaining: usize },
    Lossy { af: AddressFamily, current_count: usize, target: usize },
    Done,
}

pub enum AddressFamily {
    IPv4,
    IPv6,
}
```

Progress callbacks receive `Phase` values at each stage transition.

## Result Types

### `OptimizationResult`

```rust
pub struct OptimizationResult {
    pub entries: Vec<AggregatedEntry>,
    pub stats: OptimizationStats,
}
```

### `AggregatedEntry`

```rust
pub struct AggregatedEntry {
    pub prefix: IpNet,
    pub source_indices: Option<Vec<usize>>,  // None if source_map disabled
    pub over_coverage: u128,
    pub exclusion_collisions: Option<Vec<ExclusionCollision>>,  // None if no collisions
}
```

### `OptimizationStats`

```rust
pub struct OptimizationStats {
    pub input_ipv4_count: usize,
    pub input_ipv6_count: usize,
    pub output_ipv4_count: usize,
    pub output_ipv6_count: usize,
    pub total_ipv4_over_coverage: u128,
    pub total_ipv6_over_coverage: u128,
    pub ipv4_compression_ratio: f64,
    pub ipv6_compression_ratio: f64,
    pub ipv4_target_binding: bool,   // true if lossless output exceeded target
    pub ipv6_target_binding: bool,
    pub ipv4_exclusion_constrained: bool,  // true if exclusions prevented meeting target
    pub ipv6_exclusion_constrained: bool,
}
```

### `ReaderResult`

```rust
pub struct ReaderResult {
    pub result: OptimizationResult,
    pub input_metadata: Vec<InputEntry>,
}
```

### `InputEntry`

```rust
pub struct InputEntry {
    pub original: String,
    pub comment: Option<String>,
}
```

## Error Types

### `OptimizeError`

Returned by `optimize` and `optimize_with_progress`:

| Variant | Meaning |
|---------|---------|
| `EmptyInput` | No valid prefixes in input |
| `InvalidConfig { message }` | Configuration parameter out of valid range |
| `TargetTooSmall { target, minimum }` | Target is 0 but address family has entries |
| `InputTooLarge { count, limit }` | Input exceeds `max_input_entries` |
| `ArenaOverflow` | Trie exceeds u32::MAX nodes (extremely large inputs) |
| `Cancelled` | Progress callback returned `Break(())` |
| `CoverageLost` | Internal invariant violation (indicates a bug) |

### `OptimizerError`

Returned by `optimize_from_reader`:

| Variant | Meaning |
|---------|---------|
| `Optimize(OptimizeError)` | Wraps any `OptimizeError` |
| `Parse { line, message }` | Invalid input at specific line number |
| `Io(std::io::Error)` | I/O failure reading input |

### Error Handling Pattern

```rust
use cidr_optimizer::{optimize_from_reader, OptimizerError, OptimizeError};

match optimize_from_reader(reader, &config) {
    Ok(reader_result) => {
        let result = reader_result.result;
        // use result.entries, result.stats
    }
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

## Feature Flags

| Feature | Purpose |
|---------|---------|
| `stress` | Enables property-based stress tests (`proptest`). Test-only — does not change runtime behavior. Not relevant to library consumers. |

## Integration Examples

### Basic Usage

```rust
use cidr_optimizer::{optimize, OptimizerConfig, TargetSpec};
use ipnet::IpNet;

let prefixes: Vec<IpNet> = vec![
    "10.0.0.0/24".parse().unwrap(),
    "10.0.1.0/24".parse().unwrap(),
    "10.0.4.0/24".parse().unwrap(),
    "10.0.5.0/24".parse().unwrap(),
];

let config = OptimizerConfig {
    ipv4_target: Some(TargetSpec::EntryCount(2)),
    ..Default::default()
};

let result = optimize(&prefixes, &config).unwrap();
for entry in &result.entries {
    println!("{} (over-coverage: {} IPs)", entry.prefix, entry.over_coverage);
}
```

### Progress Reporting

```rust
use cidr_optimizer::{optimize_with_progress, OptimizerConfig, TargetSpec, Phase};
use std::ops::ControlFlow;

let result = optimize_with_progress(&prefixes, &config, |phase| {
    match phase {
        Phase::Lossless { af, entries_remaining } => {
            eprintln!("{:?}: lossless aggregation ({} entries)", af, entries_remaining);
        }
        Phase::Lossy { af, current_count, target } => {
            eprintln!("{:?}: reducing {} → {}", af, current_count, target);
        }
        Phase::Done => eprintln!("Optimization complete"),
        _ => {}
    }
    ControlFlow::Continue(())
})?;
```

### From Reader

```rust
use cidr_optimizer::{optimize_from_reader, OptimizerConfig, TargetSpec};
use std::io::BufReader;
use std::fs::File;

let file = File::open("allow-list.txt")?;
let reader = BufReader::new(file);

let config = OptimizerConfig {
    ipv4_target: Some(TargetSpec::EntryCount(1000)),
    source_map: true,
    ..Default::default()
};

let reader_result = optimize_from_reader(reader, &config)?;
for entry in &reader_result.result.entries {
    if let Some(sources) = &entry.source_indices {
        println!("{} covers {} original entries", entry.prefix, sources.len());
    }
}
```

### Over-Coverage Ratio Target

```rust
use cidr_optimizer::{optimize, OptimizerConfig, TargetSpec};

// Find minimum entries keeping over-coverage ≤ 0.1%
let config = OptimizerConfig {
    ipv4_target: Some(TargetSpec::MaxOverCoverage(0.001)),
    ..Default::default()
};

let result = optimize(&prefixes, &config)?;
println!("Reduced to {} entries", result.stats.output_ipv4_count);
```

### With Exclusion Zones

```rust
use cidr_optimizer::{optimize, ExclusionEntry, OptimizerConfig, TargetSpec};

let prefixes: Vec<ipnet::IpNet> = vec![
    "10.0.0.0/24".parse().unwrap(),
    "10.0.1.0/24".parse().unwrap(),
    "10.0.2.0/24".parse().unwrap(),
    "10.0.3.0/24".parse().unwrap(),
];

let config = OptimizerConfig {
    ipv4_target: Some(TargetSpec::EntryCount(2)),
    exclusions: vec![ExclusionEntry {
        prefix: "10.0.2.0/24".parse().unwrap(),
        source: "internal.txt".to_string(),
        comment: Some("corp network".to_string()),
    }],
    ..Default::default()
};

let result = optimize(&prefixes, &config).unwrap();
// 10.0.2.0/24 is protected — optimizer cannot merge it into a wider supernet
for entry in &result.entries {
    println!("{}", entry.prefix);
    if let Some(ref collisions) = entry.exclusion_collisions {
        for c in collisions {
            println!("  overlaps exclusion: {} ({})", c.exclusion_prefix, c.exclusion_source);
        }
    }
}
```

## `ExclusionEntry`

```rust
pub struct ExclusionEntry {
    pub prefix: IpNet,
    pub source: String,
    pub comment: Option<String>,
}
```

| Field | Type | Description |
|-------|------|-------------|
| `prefix` | `IpNet` | CIDR range to protect from absorption |
| `source` | `String` | Origin filename or identifier |
| `comment` | `Option<String>` | Optional annotation (e.g. reason for exclusion) |

## `ExclusionCollision`

```rust
pub struct ExclusionCollision {
    pub exclusion_prefix: String,
    pub exclusion_source: String,
    pub exclusion_comment: Option<String>,
}
```

| Field | Type | Description |
|-------|------|-------------|
| `exclusion_prefix` | `String` | The exclusion CIDR that intersects the output prefix |
| `exclusion_source` | `String` | Origin filename of the exclusion entry |
| `exclusion_comment` | `Option<String>` | Annotation from the exclusion entry |

Populated on `AggregatedEntry.exclusion_collisions` when an output prefix overlaps one or more exclusion ranges. `None` when there are no collisions.

## See Also

- [Architecture](ARCHITECTURE.md) — Algorithm internals, data structures, and correctness arguments
- [User Guide](USER_GUIDE.md) — CLI-equivalent behavior and output format details
