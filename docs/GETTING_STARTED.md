# Getting Started

Progressive scenarios to learn cidr-list-optimizer by doing. Each builds on the previous.

**Prerequisites**: Install per the [README](../README.md#installation).

## Scenario 1: Lossless Aggregation

**Goal**: Merge adjacent and redundant CIDRs without any over-coverage.

Create `sample.txt`:

```
10.0.0.0/25       # office-east lower half
10.0.0.128/25     # office-east upper half
10.0.1.0/24
10.0.1.0/25
192.168.1.1/32    # jump host
```

Lines starting with `#` are full-line comments (skipped entirely). Text after `#` on a CIDR line is an inline comment — captured and preserved in source-map output for traceability.

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
IPv4 over-coverage: 50.00% (1024 IPs)
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

### Alternative: Ratio Target Mode

Instead of specifying an entry count and capping over-coverage separately, you can target a specific over-coverage percentage directly:

```bash
cidr-optimizer --ipv4-target 'over-coverage=5%' large-feed.txt
```

This minimizes the number of output entries while guaranteeing over-coverage stays within 5%. The optimizer finds the smallest entry count achievable within that bound. See [User Guide — `--max-over-coverage` Behavior](USER_GUIDE.md#--max-over-coverage-behavior) for details on the conflict rule with `--max-over-coverage`.

## Scenario 7: AWS Output Format

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

## Scenario 8: Stdin Piping

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

## Scenario 9: Exclusion Zones

**Goal**: Protect specific CIDR ranges from being absorbed during lossy optimization.

Create `partners.txt`:

```
10.0.0.0/24
10.0.1.0/24
10.0.2.0/24
10.0.3.0/24
```

Create `protected.txt`:

```
10.0.2.0/24  # monitoring subnet
```

Run:

```bash
cidr-optimizer --ipv4-target 2 --max-over-coverage -1 --exclude-cidr protected.txt partners.txt
```

Expected behavior: The CLI exits with code 2:

```
error: IPv4 target unreachable — exclusion zones prevent sufficient merging
hint: reduce exclusion entries or raise the target
```

The optimizer merged 10.0.0.0/24 + 10.0.1.0/24 into 10.0.0.0/23, but could not merge further because 10.0.2.0/24 is protected by the exclusion zone. The best achievable result is 3 entries (10.0.0.0/23, 10.0.2.0/24, 10.0.3.0/24), which exceeds the target of 2.

Now try with a reachable target:

```bash
cidr-optimizer --ipv4-target 3 --max-over-coverage -1 --exclude-cidr protected.txt partners.txt
```

Expected output:

```
10.0.0.0/23
10.0.2.0/24
10.0.3.0/24
```

What happened: The exclusion zone blocked the merge that would have absorbed 10.0.2.0/24 into a wider supernet. The optimizer merged what it could (10.0.0.0/24 + 10.0.1.0/24 → /23) while respecting the exclusion. For full exclusion zone documentation, see [User Guide — Exclusion Zones](USER_GUIDE.md#exclusion-zones).

## Scenario 10: Prefix Length Clamping

**Goal**: Truncate overly-specific prefixes to a maximum length before aggregation.

Create `host-routes.txt`:

```
10.0.0.1/32
10.0.0.2/32
10.0.0.100/32
10.0.0.200/32
10.0.1.5/32
10.0.1.50/32
```

Run without clamping:

```bash
cidr-optimizer host-routes.txt
```

Output: 6 entries (no siblings to merge — scattered /32s).

Now clamp to /24:

```bash
cidr-optimizer --max-prefix-len-v4 24 --stats host-routes.txt
```

Expected output:

```
10.0.0.0/23
```

What happened: All /32 host routes were truncated to /24 before aggregation. The resulting 10.0.0.0/24 and 10.0.1.0/24 are siblings and merged losslessly into 10.0.0.0/23. This is useful when downstream systems have prefix length restrictions or when /32 granularity is unnecessary. See [User Guide](USER_GUIDE.md) for full `--max-prefix-len-v4`/`--max-prefix-len-v6` documentation.

## Scenario 11: IPv6 Prefix Length Clamping

**Goal**: Truncate overly-specific IPv6 prefixes to a maximum length before aggregation.

Create `ipv6-hosts.txt`:

```
2001:db8::1/128
2001:db8::2/128
2001:db8:0:1::1/128
2001:db8:0:1::2/128
```

Run without clamping:

```bash
cidr-optimizer ipv6-hosts.txt
```

Output: 4 entries (no siblings to merge — scattered /128s).

Now clamp to /64:

```bash
cidr-optimizer --max-prefix-len-v6 64 --stats ipv6-hosts.txt
```

Expected output:

```
2001:db8::/63
```

What happened: All /128 host routes were truncated to /64 before aggregation. The resulting 2001:db8::/64 and 2001:db8:0:1::/64 are siblings and merged losslessly into 2001:db8::/63. See [User Guide](USER_GUIDE.md) for full `--max-prefix-len-v4`/`--max-prefix-len-v6` documentation.

## Scenario 12: Preferred Over-Coverage Zones

**Goal**: Steer widening toward your own address space rather than unrelated ranges.

Create `partner-ips.txt`:

```
10.0.0.0/24
10.0.1.0/24
10.0.4.0/24
10.0.5.0/24
192.168.0.0/24
192.168.1.0/24
```

Create `our-space.txt` (address space you own and accept over-coverage into):

```
10.0.0.0/21  # our allocation
```

Run:

```bash
cidr-optimizer --ipv4-target 3 \
  --preferred-over-coverage-cidrs our-space.txt \
  --stats partner-ips.txt
```

Expected output (stdout):

```
10.0.0.0/23
10.0.4.0/23
192.168.0.0/23
```

Statistics (stderr):

```
IPv4: 6 input → 3 output (compression: 2.0x)
IPv6: 0 input → 0 output (compression: 1.0x)
```

What happened: Each pair of adjacent /24s merged losslessly into /23s — no over-coverage was needed because the target of 3 was achievable through sibling merging alone. The preferred zones become relevant when the target forces widening beyond siblings.

Now try with a tighter target to see preferred zones in action:

```bash
cidr-optimizer --ipv4-target 2 \
  --preferred-over-coverage-cidrs our-space.txt \
  --max-over-coverage -1 \
  --stats partner-ips.txt
```

Expected output:

```
10.0.0.0/21
192.168.0.0/23
```

Statistics show all over-coverage landed in preferred space:

```
IPv4: 6 input → 2 output (compression: 3.0x)
IPv6: 0 input → 0 output (compression: 1.0x)
IPv4 over-coverage: 66.67% (1024 IPs)
  Preferred: 66.67% (1024 IPs)
  Non-preferred: 0.00% (0 IPs)
```

What happened: The optimizer merged all four 10.x /24s into 10.0.0.0/21 — the 1024 extra IPs (10.0.2.0/24, 10.0.3.0/24, 10.0.6.0/24, 10.0.7.0/24) all fall within our preferred 10.0.0.0/21 allocation. Non-preferred over-coverage is zero because the 192.168.0.0/23 merge was lossless (two siblings).

Add `--max-non-preferred-over-coverage` to cap non-preferred widening:

```bash
cidr-optimizer --ipv4-target 3 \
  --preferred-over-coverage-cidrs our-space.txt \
  --max-non-preferred-over-coverage 20 \
  partner-ips.txt
```

This caps non-preferred over-coverage at 20% independently of the global cap. See [User Guide — Preferred Over-Coverage Zones](USER_GUIDE.md#preferred-over-coverage-zones) for full documentation.

## Next Steps

- [User Guide](USER_GUIDE.md) — Full CLI flag reference and all configuration options
- [Developer API](DEVELOPER_API.md) — Library crate integration reference
- [Architecture](ARCHITECTURE.md) — How the algorithm works internally

---

*This project and its documentation were fully generated using Gen AI coding tools employing multi-pass adversarial reviews to minimize errors. While this process significantly reduces defects, it cannot guarantee the complete absence of bugs.*