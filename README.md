# cidr-list-optimizer

Fit oversized IP allow-lists into AWS entry limits by computing the least-worst superset — with bounded over-exposition and full provenance tracking.

## The Problem

AWS networking services impose hard limits on allow-list entries (Security Groups: 60, NACLs: 20, Prefix Lists: 1,000, WAF IP Sets: 10,000). Real-world allow-lists from partner APIs, CDN pools, or SaaS services routinely exceed these limits. This tool computes the least-worst CIDR superset that fits your entry budget, minimizing additional IP addresses allowed while keeping every widening decision tracked and auditable.

## Features

- Lossless CIDR aggregation
- Budget-constrained lossy optimization with minimal over-exposition
- Per-address-family independent targets
- Provenance tracking
- Over-coverage percentage cap
- AWS-native output formats
- Deterministic output

## Installation

```bash
# From crates.io
cargo install cidr-optimizer-cli

# From source
cargo install --path crates/cidr-optimizer-cli

# Or build locally
cargo build --release
```

**Requirements**: Rust 1.93+ (edition 2021)

**Crates**: [cidr-optimizer](https://crates.io/crates/cidr-optimizer) (library) · [cidr-optimizer-cli](https://crates.io/crates/cidr-optimizer-cli) (binary)

## Quick Start

```bash
# Lossless aggregation (merge siblings, remove redundancies)
cidr-optimizer input.txt

# Fit partner IP ranges into a Security Group (max 60 rules)
cidr-optimizer --ipv4-target 60 --max-over-coverage 5 partner-ips.txt
```

## Performance

Approximate timings on modern hardware (Apple M-series / AMD Zen 4):

| Input Size | Lossless | Budget (target=1000) |
|-----------|----------|---------------------|
| 100K IPv4 | < 0.5s | < 1s |
| 1M IPv4 | < 2s | < 3s |
| 10M IPv4 | < 15s | < 20s |

## Project Structure

```
crates/
  cidr-optimizer/         Core library
  cidr-optimizer-cli/     CLI binary (thin wrapper)
docs/
  ARCHITECTURE.md         Algorithm internals and design
  USER_GUIDE.md           CLI/API reference and configuration
  GETTING_STARTED.md      Progressive tutorial scenarios
```

## Build & Test

| Command | Purpose |
|---------|---------|
| `cargo build` | Build all crates |
| `cargo test` | Run unit tests |
| `cargo test -p cidr-optimizer --features stress` | Run stress/property tests |
| `cargo build --release` | Optimized binary |
| `cargo clippy` | Lint checks |

## Documentation

- [Getting Started](docs/GETTING_STARTED.md) — Learn by doing with progressive scenarios
- [User Guide](docs/USER_GUIDE.md) — Full CLI reference, library API, and configuration
- [Architecture](docs/ARCHITECTURE.md) — Algorithm design, data structures, and correctness arguments

## License

MIT
