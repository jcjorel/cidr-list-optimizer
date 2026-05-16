# Changelog

## [1.2.0] 

### Added

- **Exclusion zones** — `--exclude-cidr FILE` flag (repeatable) prevents the optimizer from widening prefixes into specified ranges, protecting address space that must never appear as over-coverage (e.g., competitor networks, internal ranges)
- **Excluded-input warnings** — `--warn-on-excluded-input` reports overlaps between output CIDRs and exclusion zones
- Exit code 2 when exclusions make the entry target unreachable

### Documentation

- Document exclusion zones across all guides (README, User Guide, Developer API, Architecture, Getting Started)
- Add CloudFront optimization example table to README
- Clarify feature descriptions and budget optimization bullet in README

## [1.1.0] - 2026-05-16

### Breaking Changes

- **`--provenance` replaced by `--source-map FILE`** — source-map data is now written to a separate JSON file instead of being embedded in `--format json` output
- **`--validate` flag removed** — coverage validation is now always-on via the library internal invariant check

### Added

- **Over-coverage ratio target mode** — targets can be specified as `over-coverage=X%` to minimize entries while bounding over-exposition to a percentage
- **Inline comment support** — parser captures text after `#` on CIDR lines, preserved in source-map output

### Changed

- **Provenance renamed to source-map** — all modules, types, config fields, and CLI flags renamed for clarity

### Documentation

- Extract library API into dedicated `docs/DEVELOPER_API.md`
- Document inline comment syntax in input format section
- Update source-map format documentation
- Fix review findings in README and ARCHITECTURE
- Add crates.io install instructions and crate links to README

### Chores

- Add `publish` target to Makefile
- Fix trailing whitespace in architecture diagram

## [1.0.0] - 2026-05-15

### Added

- **CIDR list optimizer** — lossless and lossy merging modes, IPv4/IPv6 support, binary trie-based aggregation, and source-map tracking
- **CLI tool** — thin wrapper over the core library with stdin/file input and multiple output formats
- **Over-coverage cap as percentage** — `--max-over-coverage` accepts 0–1000%, exit 2 when target is unreachable, warning on uncapped excess
- **Stress and property tests** — comprehensive test suite with configurable scenarios

### Fixed

- **Parser hardening** — 4 KiB per-line length limit, capped parse warnings at 1000, truncated error messages to 100 chars
- **Optimizer overflow** — fixed `widening_mul` return type from u32 to u64, added bounds checks in merge loop and heap compaction
- **Trie stack overflow** — replaced recursive `invalidate_subtree` with iterative stack

### Documentation

- README with features, quick start, and performance table
- Architecture guide covering algorithm design and data structures
- User guide with full CLI reference and library API
- Getting started tutorial with progressive scenarios
- MIT license

### Chores

- Set MSRV to Rust 1.93
- Makefile with standard targets (`build`, `test`, `test-all`, `lint`, `install`, `clean`)
- crates.io publishing metadata
