# Feedback from Iteration 1

## Minor Issues to Fix

### 1. Stats display uses incorrect IPv4 input count calculation
- File: `crates/cidr-optimizer-cli/src/main.rs`
- Fix: Use `parsed.ipv4.len()` for IPv4 input count and `parsed.ipv6.len()` for IPv6 input count instead of the current incorrect calculation.

### 2. `--validate` flag silently ignored
- File: `crates/cidr-optimizer-cli/src/main.rs`
- Fix: Print a warning to stderr if `--validate` is used but not yet implemented, OR implement it this iteration if the optimizer is ready.

## Next Phase Direction

Iteration 1 successfully implemented the foundation (types, errors, parser) and lossless aggregation. Iteration 2 should implement:

1. **Path-compressed trie** (`trie.rs`) — arena-allocated, path-compressed binary trie as specified in the specification (§4). The trie node struct MUST be exactly 80 bytes. Path compression is critical for IPv6 /128 entries.

2. **Greedy optimizer** (`optimizer.rs`) — greedy collapse with BinaryHeap, cost-efficiency key using widening 160-bit multiplication, correct over-coverage tracking, integer-scaled ratio check for IPv6 overflow safety.

3. **Integration** — Wire the trie and optimizer into the pipeline in `lib.rs` so the CLI can use `--max-entries` to trigger lossy optimization.

Refer to `scratchpad/specification.md` §4 and `scratchpad/implementation.md` Phase 3 for exact algorithm details.
