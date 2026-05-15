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

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Internal error (coverage invariant violation — indicates a bug) |
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

For programmatic control of these parameters, see [Developer API — OptimizerConfig Fields](DEVELOPER_API.md#optimizerconfig-fields).

## Coverage Validation

Coverage validation is always performed automatically. The library verifies that every input prefix is covered by at least one output prefix before returning results. If validation fails, the CLI exits with code 1 (indicates a bug — please report it).

## Library API

For library integration (using `cidr-optimizer` as a Rust dependency), see the [Developer API Reference](DEVELOPER_API.md) which covers `OptimizerConfig`, progress callbacks, error handling, and complete integration examples.

## Output Formats

For programmatic access to these data structures (`OptimizationResult`, `AggregatedEntry`, `OptimizationStats`), see [Developer API — Result Types](DEVELOPER_API.md#result-types).

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

When `--source-map <FILE>` is specified, the detailed source mapping is written to the given file in JSON format (regardless of `--format`). For programmatic access to source-map data, see [Developer API — AggregatedEntry](DEVELOPER_API.md#result-types).

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

### Entry Fields

Each entry in the `entries` array contains:

| Field | Type | Description |
|-------|------|-------------|
| `prefix` | string | Output CIDR (e.g. `"10.0.0.0/22"`) |
| `sources` | array | Original inputs covered by this output prefix |
| `over_coverage` | integer | IPs in this prefix not covered by any input |

Each element in `sources`:

| Field | Type | Description |
|-------|------|-------------|
| `index` | integer | Zero-based position in the input (line order, skipping comments/blanks) |
| `cidr` | string | Original CIDR as parsed from input |
| `comment` | string or null | Inline comment from the input line (text after the CIDR), or `null` if none |

### Interpretation

- `sources` lists every original input entry that is contained within the output `prefix`
- `over_coverage = 0` means the output prefix exactly covers its inputs (lossless)
- `over_coverage > 0` means the prefix was widened during lossy optimization, including IPs not in the original input
- `comment` preserves inline annotations from the input file, enabling traceability back to the source (e.g. partner name, ticket ID)

### Production Usage

- **Audit trail**: Map firewall rules back to original allow-list entries using `sources[].cidr` and `sources[].comment`
- **Incremental updates**: When an input entry is removed, search `sources[].index` to identify which output CIDRs are affected
- **Cost analysis**: Rank output prefixes by `over_coverage` to understand aggregation cost

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
# CDN provider ranges
10.0.0.0/24       # partner-A
192.168.1.0/25    # ticket JIRA-1234
2001:db8::/32
192.168.1.1
```

Rules:
- Lines starting with `#` are full-line comments (skipped)
- Text after `#` on a CIDR line is an inline comment (preserved in source-map output)
- Blank lines are skipped
- Bare IP addresses without a prefix length are treated as host addresses (`/32` for IPv4, `/128` for IPv6)
- Non-canonical CIDRs (host bits set) are normalized with a warning: `10.0.0.5/24` → `10.0.0.0/24`
- Maximum line length: 4096 bytes
- Entry indices for source-map are assigned sequentially, counting only valid entries (not comments/blanks)

## Integration Patterns

For library integration (using `cidr-optimizer` as a Rust dependency), see the [Developer API Reference](DEVELOPER_API.md).

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
# Optimize and verify success (exit 1 = internal bug, exit 2 = target unreachable)
cidr-optimizer --ipv4-target 1000 allow-list.txt > optimized.txt
echo "Optimization succeeded (exit $?)"
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
