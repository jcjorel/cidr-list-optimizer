---
name: changelog-management
description: 'Manages CHANGELOG.md updates using git history and version state derived from Cargo.toml and git tags. Use when reading, writing, improving or reviewing CHANGELOG.md. Do NOT use for general documentation or non-changelog files.'
---

The keywords MUST, MUST NOT, REQUIRED, SHALL, SHALL NOT, SHOULD, SHOULD NOT, RECOMMENDED, NOT RECOMMENDED, MAY, and OPTIONAL in this document are to be interpreted as described in [RFC 2119](https://www.rfc-editor.org/rfc/rfc2119) and [RFC 8174](https://www.rfc-editor.org/rfc/rfc8174).

## Critical Constraints

- MUST derive the current version from `crates/cidr-optimizer/Cargo.toml` `[package] version` field (the library crate is authoritative).
- MUST determine release state by checking if a git tag `v<version>` exists for the latest CHANGELOG entry.
- MUST use [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) format.
- MUST use [Semantic Versioning](https://semver.org/).
- MUST NOT fabricate entries — all content MUST be traceable to git history.

## Version State Detection

1. Read the version from `crates/cidr-optimizer/Cargo.toml` (`version = "X.Y.Z"`).
2. Parse the first `## [X.Y.Z]` entry in `CHANGELOG.md`.
3. Run `git tag -l "v<version>"` for the version in the latest CHANGELOG entry.
4. Determine state:
   - If tag exists → version is **released**. New changes go into a new unreleased section.
   - If tag does NOT exist → version is **unreleased**. New changes append to the existing section.

## Adding Entries

1. Run version state detection (above).
2. Gather commits since the last tagged version:
   ```bash
   git log v<last-tagged-version>..HEAD --oneline
   ```
   - If no tag exists at all, use `git log --oneline`.
3. Classify commits using Conventional Commits prefixes:

| Prefix | CHANGELOG Section |
|--------|-------------------|
| `feat` | Added |
| `fix` | Fixed |
| `docs` | Documentation |
| `refactor` | Changed |
| `perf` | Changed |
| `chore` | Chores |
| `BREAKING CHANGE` or `!` | Breaking Changes |

4. If version state is **unreleased**: merge new entries into the existing `## [X.Y.Z]` section.
5. If version state is **released**: create a new `## [X.Y.Z]` section at the top (below `# Changelog`), using today's date only when the user explicitly releases.
6. MUST NOT duplicate entries already present in the CHANGELOG.
7. MUST write human-readable descriptions, not raw commit messages. Group related commits into single entries where appropriate.

## Format Rules

- Header: `# Changelog`
- Version entry: `## [X.Y.Z] - YYYY-MM-DD` (released) or `## [X.Y.Z]` (unreleased, no date)
- Subsections: `### Added`, `### Changed`, `### Fixed`, `### Breaking Changes`, `### Documentation`, `### Chores`, `### Infrastructure`
- Each entry: `- **Bold summary** — description` or `- Description` for simple items.
- MUST keep entries concise — one line per logical change.
- MUST place Breaking Changes section first when present.
- MUST include a Migration Guide subsection for breaking changes when the migration path is non-obvious.

## Verification

After modifying CHANGELOG.md:
1. Confirm the version in the first `## [...]` entry matches or exceeds the version in `crates/cidr-optimizer/Cargo.toml`.
2. Confirm no duplicate entries exist.
3. Confirm date is absent for unreleased versions, present for released ones.
