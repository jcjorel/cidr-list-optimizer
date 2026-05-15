---
name: git-commit
description: Enforces state-of-the-art Git commit message conventions (Conventional Commits + Chris Beams' 7 rules) with sensitive data protection. Use when creating a Git commit, writing a commit message, or staging changes for commit. Do NOT use for general Git operations like branching, merging, or rebasing.
---

# Git Commit Message Skill

The keywords MUST, MUST NOT, REQUIRED, SHALL, SHALL NOT, SHOULD, SHOULD NOT, RECOMMENDED, NOT RECOMMENDED, MAY, and OPTIONAL in this document are to be interpreted as described in [RFC 2119](https://www.rfc-editor.org/rfc/rfc2119) and [RFC 8174](https://www.rfc-editor.org/rfc/rfc8174).

## Critical Constraints

- Subject line MUST be ≤50 characters. MUST NOT exceed 72 characters under any circumstance.
- Subject MUST use imperative mood — completes: "If applied, this commit will ___".
- Subject MUST NOT end with a period.
- First letter after `type(scope): ` MUST be uppercase.
- Body MUST wrap at 72 characters per line.
- Each commit MUST contain exactly ONE logical change.
- Commit message (subject, body, footers) MUST NOT contain sensitive data. See §Sensitive Data in Commit Messages.

## Sensitive Data in Commit Messages

The commit message MUST NEVER contain any of the following:

| Category | Examples |
|----------|----------|
| Credentials | API keys, tokens, passwords, secrets, private keys, certificates |
| PII | Email addresses, phone numbers, full names of non-public individuals, national IDs, dates of birth |
| Network identifiers | IP addresses (v4/v6), internal hostnames, private DNS names |
| Cloud identifiers | AWS account IDs, ARNs, resource IDs (e.g., `i-0abc`, `arn:aws:`, `AKIA`), GCP project IDs, Azure subscription IDs |
| Financial | Credit card numbers, bank account numbers, IBAN |
| Internal paths | Absolute filesystem paths containing usernames or org-specific structure |

When describing changes that involve such data:
- MUST use generic placeholders: "Update connection string", not "Set password to X".
- MUST reference by category: "Rotate API key for payment service", not the key itself.
- MUST use ticket/issue references for traceability instead of embedding identifiers.

## Sensitive Data in Staged Files

Before committing, MUST scan `git diff --cached` output for sensitive data patterns:

| Pattern | Regex hint |
|---------|-----------|
| AWS access key ID | `AKIA[0-9A-Z]{16}` |
| AWS secret / generic secret | `(?i)(secret\|password\|token\|api_key\|apikey)\s*[:=]\s*\S+` |
| Private key header | `-----BEGIN (RSA\|EC\|DSA\|OPENSSH) PRIVATE KEY-----` |
| IPv4 address | `\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b` (excluding `0.0.0.0`, `127.0.0.1`, `255.255.255.255`) |
| AWS ARN | `arn:aws[a-z\-]*:[a-z0-9\-]+:[a-z0-9\-]*:\d{12}:` |
| AWS account ID (standalone 12-digit) | `\b\d{12}\b` in context of AWS config/code |
| Email address | `[a-zA-Z0-9._%+\-]+@[a-zA-Z0-9.\-]+\.[a-zA-Z]{2,}` |
| Generic high-entropy string (≥32 chars, base64/hex) | `[A-Za-z0-9+/=]{32,}` or `[0-9a-f]{32,}` in assignment context |

Procedure when sensitive data is detected in staged files:

1. MUST list each finding: file path, line number, pattern category, and a redacted preview.
2. MUST ask the user: "Sensitive data detected in staged files. Do you want to proceed with the commit?"
3. - If user confirms → proceed with commit (sensitive data in *files* may be intentional, e.g., test fixtures, documentation examples).
   - If user declines → abort and suggest remediation (`.gitignore`, environment variables, secrets manager).
4. MUST NOT silently commit when sensitive data is detected — explicit user acknowledgment is REQUIRED.

Exceptions that SHOULD NOT trigger warnings:
- Files matching `*test*`, `*mock*`, `*fixture*`, `*example*` with clearly fake data (e.g., `000000000000`, `AKIAIOSFODNN7EXAMPLE`).
- `.env.example` or `.env.template` files with placeholder values.
- Documentation files showing redacted examples.

## Commit Message Format

```
<type>(<scope>): <subject>

[body]

[footer(s)]
```

## Type Selection

| Type | When to use |
|------|-------------|
| `feat` | New feature or capability |
| `fix` | Bug fix |
| `docs` | Documentation only |
| `style` | Formatting, whitespace, semicolons (no logic change) |
| `refactor` | Code restructuring (no new feature, no bug fix) |
| `perf` | Performance improvement |
| `test` | Adding or fixing tests |
| `build` | Build system, dependencies, packaging |
| `ci` | CI/CD pipeline configuration |
| `chore` | Maintenance tasks, tooling |
| `revert` | Reverts a previous commit |

## Scope

The `(scope)` is OPTIONAL but RECOMMENDED. Use the project's common vocabulary (e.g., `cdk`, `joiner`, `helper`, `api`).

## Breaking Changes

Indicate with `!` after type/scope AND/OR a `BREAKING CHANGE:` footer:

```
feat(api)!: Remove deprecated v1 endpoints

BREAKING CHANGE: /v1/users has been removed. Migrate to /v2/users.
```

## Footer Conventions

| Footer | Purpose |
|--------|---------|
| `Closes #123` or `Fixes #123` | Closes an issue |
| `Refs: #456, #789` | References without closing |
| `BREAKING CHANGE: <description>` | Breaking API change |
| `Co-authored-by: Name <email>` | Credit co-authors |

## Procedure

1. MUST run `git diff --cached` to review staged changes.
2. MUST scan diff output for sensitive data patterns (see §Sensitive Data in Staged Files). If detected, MUST warn and obtain user permission before continuing.
3. MUST verify atomicity — if multiple unrelated changes are staged, split into separate commits.
4. MUST select the type from the type table based on the nature of the change.
5. SHOULD determine scope — identify the affected component/module.
6. MUST write the subject in imperative mood, ≤50 chars, no period, capitalized after prefix.
7. MUST verify the commit message contains no sensitive data (see §Sensitive Data in Commit Messages).
8. SHOULD write a body if the change is non-trivial — explain what changed and why (not how).
9. SHOULD add footers — issue references, breaking change notices, co-authors.
10. MUST validate all critical constraints are satisfied before committing.
11. MUST run `git log -1 --format="%s"` after committing and verify subject length ≤50 chars and imperative mood.

## Examples

Simple fix (no body needed):

```
fix(parser): Handle empty input without crashing
```

Feature with body:

```
feat(auth): Add OAuth2 login via Google

Users can now authenticate using their Google account instead of
only email/password. This reduces signup friction for enterprise
customers who already use Google Workspace.

Closes #234
```

Breaking change:

```
feat(api)!: Return paginated responses from list endpoints

All list endpoints now return `{ items: [], nextToken: string }`
instead of a plain array. This enables efficient pagination for
large datasets.

BREAKING CHANGE: List endpoint response shape changed from array
to object with `items` and `nextToken` fields.

Refs: #567
```

## Anti-Patterns

| Bad | Reason |
|-----|--------|
| `fixed bug` | No type, past tense, vague |
| `feat: updated the login page.` | Past tense, period at end |
| `WIP` | MUST NOT commit work-in-progress to shared branches |
| `misc changes` | Meaningless, not atomic |
| Subject >72 chars | Truncated by GitHub |
| Mixed unrelated changes | Breaks bisect/revert |
| `fix: Set DB password to hunter2` | Leaks credentials in commit history |
| `feat: Connect to 10.0.3.45:5432` | Leaks internal IP in commit history |
| `chore: Update key AKIA3E...` | Leaks AWS access key in commit history |
