# Getting Started

Progressive scenarios to learn cidr-list-optimizer by doing. Each builds on the previous.

**Prerequisites**: Install per the [README](../README.md#installation).

## Scenario 1: Lossless Aggregation

**Goal**: Merge adjacent and redundant CIDRs without any over-coverage.

Create `sample.txt`:

```
10.0.0.0/25
10.0.0.128/25
10.0.1.0/24
10.0.1.0/25
192.168.1.1/32
```

Run:

```bash
cidr-optimizer sample.txt
```

Expected output:

```
10.0.0.0/23
192.168.1.1/32
```

What happened: The two /25 siblings merged into 10.0.0.0/24, which then merged with 10.0.1.0/24 into 10.0.0.0/23. The redundant 10.0.1.0/25 (already covered by 10.0.1.0/24) was eliminated. The isolated /32 passed through unchanged.

## Scenario 2: IPv4 Budget Mode

**Goal**: Reduce output to fit a Security Group (60 rules max) or similar entry budget.

Create `large-feed.txt` with non-adjacent prefixes (simulating a partner IP allow-list):

```
10.0.0.0/24
10.0.1.0/24
10.0.4.0/24
10.0.5.0/24
10.0.8.0/24
172.16.0.0/24
172.16.1.0/24
172.16.4.0/24
```

Run:

```bash
cidr-optimizer --ipv4-target 4 --stats large-feed.txt
```

Expected output (stdout):

```
10.0.0.0/21
10.0.8.0/24
172.16.0.0/23
172.16.4.0/24
```

Statistics (stderr):

```
IPv4: 8 input → 4 output (compression: 2.0x)
IPv6: 0 input → 0 output (compression: 1.0x)
IPv4 over-coverage: 1024 IPs
```

What happened: With a target of 4, the optimizer merged non-adjacent prefixes into wider CIDRs, accepting some over-coverage (IPs not in the original input that are now included). See [User Guide — `--max-over-coverage` Behavior](USER_GUIDE.md#--max-over-coverage-behavior) for how the default cap works.

## Scenario 3: IPv6 Budget Mode

**Goal**: Optimize IPv6 prefixes independently.

Create `v6-feed.txt`:

```
2001:db8::/48
2001:db8:1::/48
2001:db8:2::/48
2001:db8:3::/48
```

Run:

```bash
cidr-optimizer --ipv6-target 2 --stats v6-feed.txt
```

Expected output:

```
2001:db8::/46
```

What happened: Four /48 siblings merged into a single /46. The target was 2 but lossless already produced 1 entry (four /48s form two sibling pairs that cascade-merge).

## Scenario 4: Mixed Address Families

**Goal**: Process IPv4 and IPv6 together with independent targets.

Create `mixed.txt`:

```
10.0.0.0/24
10.0.1.0/24
10.0.2.0/24
10.0.3.0/24
2001:db8::/48
2001:db8:1::/48
```

Run:

```bash
cidr-optimizer --ipv4-target 2 --ipv6-target 1 --stats mixed.txt
```

Expected output:

```
10.0.0.0/22
2001:db8::/47
```

What happened: IPv4 entries merged losslessly into a /22 (4 siblings). IPv6 entries merged losslessly into a /47. Each AF was optimized independently against its own target.

## Scenario 5: Source-Map Inspection

**Goal**: Understand which inputs map to each output prefix.

Create `audit.txt`:

```
10.0.0.0/24
10.0.1.0/24
10.0.2.5/32
10.0.3.0/25
```

Run:

```bash
cidr-optimizer --ipv4-target 1 --source-map /tmp/source-map.json --format json audit.txt
```

Expected stdout output (abbreviated):

```json
{
  "ipv4": [
    {
      "prefix": "10.0.0.0/22",
      "source_count": 4,
      "over_coverage": 383
    }
  ],
  "ipv6": [],
  "stats": { ... }
}
```

The source-map file (`/tmp/source-map.json`) contains the detailed mapping:

```json
{
  "entries": [
    {
      "prefix": "10.0.0.0/22",
      "sources": [
        {"index": 0, "cidr": "10.0.0.0/24", "comment": null},
        {"index": 1, "cidr": "10.0.1.0/24", "comment": null},
        {"index": 2, "cidr": "10.0.2.5/32", "comment": null},
        {"index": 3, "cidr": "10.0.3.0/25", "comment": null}
      ],
      "over_coverage": 383
    }
  ],
  "stats": { ... }
}
```

What happened: All 4 inputs were merged into a single /22. The source-map file shows which input lines (by zero-based index) are covered by each output prefix. Over-coverage of 383 means 383 IPs in the /22 were not in any original input. See [User Guide — Source-Map Interpretation](USER_GUIDE.md#source-map-interpretation) for full field definitions.

## Scenario 6: Ratio-Capped Mode

**Goal**: Limit over-coverage to protect against excessive collateral inclusion.

Using the same `large-feed.txt` from Scenario 2:

```bash
cidr-optimizer --ipv4-target 1 --max-over-coverage 5 large-feed.txt
```

Expected behavior: The CLI exits with code 2:

```
error: IPv4 target 1 unreachable — got 5 entries (over-coverage cap prevents further merging)
hint: raise the cap with --max-over-coverage <percentage> (up to 1000) or disable it with --max-over-coverage -1
```

What happened: A 5% over-coverage cap is too restrictive to merge 8 scattered /24s into a single prefix. The optimizer stopped merging when further collapses would exceed the cap. See [User Guide — `--max-over-coverage` Behavior](USER_GUIDE.md#--max-over-coverage-behavior) for details.

Now try with a higher cap:

```bash
cidr-optimizer --ipv4-target 1 --max-over-coverage -1 large-feed.txt
```

This disables the cap entirely, allowing the optimizer to merge as aggressively as needed.

## Scenario 7: Validation

**Goal**: Verify that optimization didn't lose coverage (safety check for CI/CD).

Using the same `large-feed.txt` from Scenario 2:

```bash
cidr-optimizer --ipv4-target 4 --validate large-feed.txt
```

Expected output (stderr):

```
Validation passed: all inputs covered
```

What happened: The `--validate` flag performs an independent verification that every input prefix is contained by at least one output prefix. Exits with code 1 if coverage is lost (indicates a bug). Useful as a CI gate.

## Scenario 8: AWS Output Format

**Goal**: Generate output ready for AWS CLI consumption.

Using the same `large-feed.txt` from Scenario 2:

```bash
cidr-optimizer --ipv4-target 4 --format aws large-feed.txt
```

Output:

```json
[
  {"Cidr": "10.0.0.0/21"},
  {"Cidr": "10.0.8.0/24"},
  {"Cidr": "172.16.0.0/23"},
  {"Cidr": "172.16.4.0/24"}
]
```

Pipe directly to AWS CLI:

```bash
cidr-optimizer --ipv4-target 1000 --format aws feed.txt \
  | aws ec2 modify-managed-prefix-list \
      --prefix-list-id pl-0123456789abcdef0 \
      --current-version 1 \
      --add-entries file:///dev/stdin
```

## Scenario 9: Stdin Piping

**Goal**: Process input from a pipeline without intermediate files.

```bash
curl -s https://feeds.example.com/partner-ips.txt \
  | cidr-optimizer --ipv4-target 500 --stats
```

Expected output (stderr, stdout depends on feed content):

```
IPv4: <N> input → 500 output (compression: <X>x)
IPv6: 0 input → 0 output (compression: 1.0x)
IPv4 over-coverage: <N> IPs
```

Or combine multiple feeds:

```bash
cat feed1.txt feed2.txt feed3.txt | cidr-optimizer --ipv4-target 1000
```

What happened: When no file argument is given (or `-` is passed explicitly), the CLI reads from stdin. This lets you pipe from `curl`, `cat`, or any command without intermediate files.

## Next Steps

- [User Guide](USER_GUIDE.md) — Full CLI flag reference, library API, and all configuration options
- [Architecture](ARCHITECTURE.md) — How the algorithm works internally
