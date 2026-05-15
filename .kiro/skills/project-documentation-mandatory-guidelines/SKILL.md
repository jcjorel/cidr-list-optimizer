---
name: project-documentation-mandatory-guidelines
description: Prevents content duplication and misplacement across project documentation files. Use when reading, writing, creating, improving or reviewing README.md, docs/ARCHITECTURE.md, docs/USER_GUIDE.md, or docs/GETTING_STARTED.md. Do NOT use for general documentation writing.
---

The keywords MUST, MUST NOT, REQUIRED, SHALL, SHALL NOT, SHOULD, SHOULD NOT, RECOMMENDED, NOT RECOMMENDED, MAY, and OPTIONAL in this document are to be interpreted as described in [RFC 2119](https://www.rfc-editor.org/rfc/rfc2119) and [RFC 8174](https://www.rfc-editor.org/rfc/rfc8174).

A fact MUST appear in at most ONE file; cross-reference links replace repetition.

## Scope

This skill governs ONLY the 4 files listed in the File Ownership Zones table. Content that does not belong to any zone (e.g., CONTRIBUTING.md, LICENSE, benchmarks/) is OUT OF SCOPE — proceed without applying these rules.

## Duplication Taxonomy

| Type | Definition | Rule |
|------|-----------|------|
| Verbatim duplication | Same sentence in multiple files | ALWAYS forbidden |
| Semantic duplication | Same fact, different words | Forbidden — use cross-reference instead |
| Contextual echo | Same concept mentioned in passing (≤1 sentence) to support a different point | Permitted IF it includes a cross-reference to the authoritative source |
| Example overlap | Same command appears in multiple files | Permitted ONLY IF surrounding context serves a different pedagogical purpose |

## Disambiguation Principle

When in doubt about which file owns content, apply: "Is this answering WHY (→ ARCHITECTURE), HOW-TO-USE (→ USER_GUIDE), LEARN-BY-DOING (→ GETTING_STARTED), or WHAT-IS-THIS-PROJECT (→ README)?" Place the authoritative definition in the primary owner; use contextual echoes (≤1 sentence + link) in secondary files.

## Procedure Selection

- If the user asks to REVIEW, AUDIT, or CHECK documentation → MUST follow the Documentation Review Procedure.
- If the user asks to WRITE, EDIT, IMPROVE, or UPDATE a specific file that already exists → MUST follow the Enforcement Procedure.
- If the user asks to CREATE, GENERATE, BOOTSTRAP, or INITIALIZE documentation, OR if the target file does not exist → MUST follow the Bootstrap Procedure.

## Enforcement Procedure

1. MUST identify which ownership zone (see table below) the new content belongs to.
2. If content crosses a boundary, MUST move it to the correct file.
3. MUST use cross-references (`See [Architecture](docs/ARCHITECTURE.md)`) instead of duplicating.
4. MUST verify no content appears in more than one file after the edit.

## Bootstrap Procedure

1. MUST create `docs/` directory if it does not exist.
2. MUST create files in this order: README.md first, then docs/ARCHITECTURE.md, docs/USER_GUIDE.md, docs/GETTING_STARTED.md.
3. Each file MUST contain at minimum: headings corresponding to its MUST CONTAIN items from the ownership table.
4. Cross-reference links MUST only point to files that already exist. When a new governed file is created, previously created files MUST be updated to include the link.
5. MUST apply the Enforcement Procedure rules to all content written during bootstrap.

## Cross-Reference Rules

| Rule | Constraint |
|------|-----------|
| README → other docs | MUST link to all docs that currently exist in a "Documentation" section at the bottom |
| GETTING_STARTED.md → README | MAY reference for install prerequisites |
| GETTING_STARTED.md → USER_GUIDE.md | MUST link for full CLI/API details instead of duplicating |
| GETTING_STARTED.md → ARCHITECTURE.md | MAY link for "how the algorithm works" explanations |
| ARCHITECTURE.md → README | MUST NOT link back |
| ARCHITECTURE.md → USER_GUIDE.md | MAY reference for "how to configure" details |
| ARCHITECTURE.md → GETTING_STARTED.md | MAY reference for "see tutorial" pointers |
| USER_GUIDE.md → README | MUST NOT link back |
| USER_GUIDE.md → ARCHITECTURE.md | MAY reference for "why it works this way" explanations |
| USER_GUIDE.md → GETTING_STARTED.md | MAY reference for "see tutorial" pointers |

## File Ownership Zones

| File | Role | MUST contain ONLY | MUST NOT contain |
|------|------|-------------------|------------------|
| `README.md` | Why and How to Start | Value proposition; feature bullet list (one-liner each, no impl details); prerequisites (Rust toolchain version, MSRV); install commands (`cargo install`); minimal usage example (one lossless, one budget); performance summary table (benchmark numbers only, no actionable recommendations); project structure tree (flat listing, ≤8-word noun phrase per entry, no verbs or behavior descriptions); build/test command table; links to other docs | Algorithm internals (trie, greedy, radix sort); full CLI flag reference; library API signatures; output format JSON schemas; complexity analysis; memory budget tables; configuration parameter details; step-by-step tutorials; performance tuning recommendations; module responsibility descriptions |
| `docs/GETTING_STARTED.md` | Learn by Doing | Progressive scenarios (lossless aggregation, IPv4 budget mode, IPv6 budget mode, mixed AF, provenance inspection, ratio-capped mode, AWS output format, stdin piping); each scenario with goal statement, complete runnable command, sample input file content, expected output, and "what just happened" explanation (≤2 sentences for provenance/error scenarios, linking to USER_GUIDE for full details); prerequisite checklist referencing README install steps | Value proposition / feature overview; algorithm internals; full CLI reference (link to User Guide instead); library API reference; complexity analysis; memory budget; architectural decisions; error type catalogs or exhaustive error handling guidance; complete output format schemas; configuration parameter tables; generic provenance field definitions |
| `docs/ARCHITECTURE.md` | How It Works Inside | System diagram (ASCII/Mermaid showing phases); algorithm phases (parse, radix sort, lossless aggregation, path-compressed trie, greedy lossy optimization); data structures (trie node layout, arena allocation, BinaryHeap entry); complexity analysis table; memory budget table; cost-efficiency greedy rationale; path compression explanation; over-coverage tracking math; ratio check integer arithmetic; correctness argument; references to academic papers; crate structure with module responsibilities (complete sentences explaining what each module DOES, interface boundaries, and relationships); mathematical model behind configuration parameters (without listing defaults or valid ranges) | Value proposition; install commands; CLI flag reference; output format examples; step-by-step tutorials; library API usage examples; parameter default values or valid ranges; CLI usage examples; output format schemas; benchmark numbers or performance measurements |
| `docs/USER_GUIDE.md` | How to Use It Day-to-Day | Full CLI reference (all flags with descriptions and defaults); library API reference (`optimize`, `optimize_with_progress`, `optimize_from_reader` with signatures and usage); `OptimizerConfig` fields table (type, default, valid range, behavioral effect — no mathematical rationale); `Phase` and `AddressFamily` enum documentation for progress callbacks; output formats (plain, JSON, AWS) with full schema examples; error types (`OptimizeError`, `OptimizerError`) and handling; provenance output interpretation (generic field definitions, all possible values, production usage patterns); `--validate` flag behavior; `--stats` output format; integration patterns (as library dependency, as CLI in scripts, in CI/CD pipelines); performance tuning tips (actionable guidance, recommended values, tradeoff advice) | Value proposition; install commands; algorithm internals (trie structure, greedy math); complexity proofs; academic references; step-by-step tutorials; module dependency diagrams; data structure memory layouts; benchmark numbers |

## Documentation Review Procedure

### Change Discovery (Pre-Step)

MUST execute before spawning review subagents. MUST run these commands in the main agent context:

1. Get last modification date of each doc file:

```bash
git log -1 --format="%aI %H" -- README.md
git log -1 --format="%aI %H" -- docs/GETTING_STARTED.md
git log -1 --format="%aI %H" -- docs/ARCHITECTURE.md
git log -1 --format="%aI %H" -- docs/USER_GUIDE.md
```

2. If any command returns empty (file not yet committed or no git history), set `{LAST_MODIFIED}` for that file to "File has never been committed." If the repository has no commits at all, SKIP the Change Discovery pre-step entirely and proceed with a full review using `{RECENT_CHANGES}` = "No git history available — treating all content as new."

3. Compute the oldest date among the non-empty results. Use it as `{SINCE_DATE}`.

4. Get code commits since `{SINCE_DATE}` (excluding doc files):

```bash
git log --oneline --since="{SINCE_DATE}" -- . ':!README.md' ':!docs/'
```

5. Get changed files summary:

```bash
git diff --stat $(git log -1 --format=%H --before="{SINCE_DATE}")..HEAD -- . ':!README.md' ':!docs/'
```

6. Format the output as `{RECENT_CHANGES}` — a concise list of commit subjects and affected files grouped by area (library core, CLI, tests, config). If `{LAST_MODIFIED}` is "File has never been committed" for a file, set `{RECENT_CHANGES}` for that file's subagent to "All codebase history (file is new/uncommitted)."

7. If `CHANGELOG.md` exists, read it in full and extract `{LATEST_VERSION}` and `{LATEST_CHANGELOG}`.

- If step 4 returns no commits, SHOULD inform the user that documentation is up-to-date with the codebase and ask whether to proceed with a full review anyway.

### Subagent Stages

MUST spawn parallel subagents using the `subagent` tool with mode `blocking`. MUST NOT review files sequentially in the main agent context.

MUST use five parallel stages with no dependencies between them. MUST skip stages for files that do not exist.

| Stage name | File | Role (from ownership table) |
|------------|------|----------------------------|
| readme-review | README.md | Why and How to Start |
| getting-started-review | docs/GETTING_STARTED.md | Learn by Doing |
| architecture-review | docs/ARCHITECTURE.md | How It Works Inside |
| user-guide-review | docs/USER_GUIDE.md | How to Use It Day-to-Day |
| link-validation | All existing doc files | Cross-reference integrity |

### Per-File Review Prompt

Each subagent MUST receive this prompt (substitute `{FILE}`, `{ROLE}`, `{MUST_CONTAIN}`, `{MUST_NOT_CONTAIN}` from the File Ownership Zones table, `{LAST_MODIFIED}`, `{RECENT_CHANGES}` from the Change Discovery pre-step):

```
Read {FILE} in full. Evaluate against these criteria:

CONTEXT — Codebase changes since {FILE} was last updated ({LAST_MODIFIED}):
{RECENT_CHANGES}

1. OWNERSHIP VIOLATIONS: List any content that belongs in a different file per these rules:
   - {FILE} role: {ROLE}
   - MUST contain ONLY: {MUST_CONTAIN}
   - MUST NOT contain: {MUST_NOT_CONTAIN}

2. CROSS-REFERENCE COMPLIANCE: Check links follow the cross-reference rules:
   - README → other docs: MUST link in "Documentation" section
   - GETTING_STARTED → USER_GUIDE: MUST link for full CLI/API details
   - ARCHITECTURE/USER_GUIDE → README: MUST NOT link back
   - ARCHITECTURE ↔ USER_GUIDE: MAY cross-reference
   - ARCHITECTURE/USER_GUIDE → GETTING_STARTED: MAY reference

3. CONTENT QUALITY: Flag stale information, broken links, inconsistencies with source code.

4. DUPLICATION CANDIDATES: Extract key phrases/facts that might also appear in other docs. Apply the Duplication Taxonomy: verbatim and semantic duplication are forbidden; contextual echoes (≤1 sentence + link) are permitted.

5. STALENESS: Based on the recent changes above, flag documentation sections that are likely outdated or missing coverage for new/modified features. For each finding, cite the relevant commit(s).

Output a structured report with sections: VIOLATIONS, CROSS-REFS, QUALITY, DUPLICATION_CANDIDATES, STALENESS.
```

### Cross-Reference Link Validation Prompt

```
Read ALL existing documentation files from:
- README.md
- docs/GETTING_STARTED.md
- docs/ARCHITECTURE.md
- docs/USER_GUIDE.md

(Skip any that do not exist.)

For every markdown link (`[text](target)` or `[text](target#anchor)`):

1. BROKEN FILE LINKS: Report links where the target file does not exist.
2. BROKEN ANCHOR LINKS: Report links with `#anchor` fragments where no matching heading exists in the target file.
3. ORPHANED CROSS-REFERENCES: Report any mandatory links from the Cross-Reference Rules table that are missing (only for files that currently exist).

Output a structured report with sections: BROKEN_FILE_LINKS, BROKEN_ANCHOR_LINKS, MISSING_MANDATORY_LINKS.
Each entry MUST include: source file, line content, target, and reason.
If all links are valid, output "No discrepancies found."
```

### Verification

- If violations are found, MUST present the consolidated report to the user before making changes.
- MUST NOT auto-fix without user confirmation.
