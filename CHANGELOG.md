# Changelog

## [1.1.0]

### Documentation

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
