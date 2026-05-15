# Implementation Request

Implement the CIDR List Optimizer Rust project as specified in `scratchpad/specification.md` and `scratchpad/implementation.md`.

The project is a Rust workspace with two crates:
- `crates/cidr-optimizer` — library crate (primary deliverable)
- `crates/cidr-optimizer-cli` — CLI binary crate

The workspace Cargo.toml already exists at the project root. No crate source files exist yet.

## What to implement

Follow the implementation plan in `scratchpad/implementation.md` exactly. The implementation phases are:

1. **Foundation**: Workspace setup, `types.rs`, `error.rs`, `parser.rs`, basic CLI skeleton
2. **Lossless Aggregation**: `lossless.rs` with radix sort, redundancy elimination, sibling merging, max_prefix_len enforcement, provenance tracking
3. **Path-Compressed Trie + Greedy Optimizer**: `trie.rs` (arena-allocated path-compressed binary trie), `optimizer.rs` (greedy collapse with BinaryHeap, cost-efficiency key, correct over-coverage tracking)
4. **Provenance + Output**: `provenance.rs`, full pipeline in `lib.rs`, JSON/AWS output formats, `--validate` flag
5. **Testing**: Unit tests, integration tests, property tests, differential tests vs `ipnet::aggregate()`

## Key constraints

- All code MUST compile and pass `cargo test`
- Follow the exact API signatures from the specification (§6)
- Use the exact dependencies listed in the implementation plan (§2)
- Implement the algorithm exactly as described in the specification (§4)
- The trie node struct MUST be exactly 80 bytes as specified
- Use cost-efficiency key with widening 160-bit multiplication for the BinaryHeap
- Integer-scaled ratio check for IPv6 overflow safety
- Path compression is critical for IPv6 /128 entries

## Priority

Focus on getting a working, compiling implementation with correct lossless aggregation and the greedy optimizer. Tests are important but secondary to having the core logic correct and compiling.
