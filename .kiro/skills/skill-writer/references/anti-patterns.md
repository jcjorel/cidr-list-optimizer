# Skill Anti-Patterns Reference

## Description Anti-Patterns

### Workflow summary in description (most damaging)
❌ "Analyzes git diff, identifies the change type, generates a commit message in conventional format with scope detection"
✅ "Use when generating commit messages or reviewing staged changes. Handles conventional commits, scope detection, and breaking change notation."

### Too vague
❌ "Helps with data processing and analysis tasks"
✅ "Use when querying BigQuery, writing SQL for analytics pipelines, or optimizing query performance. Handles partitioned tables, materialized views, and cost estimation."

### Wrong voice
❌ "I can help you create database migrations"
✅ "Generates database migrations for Drizzle ORM with SQLite"

## Body Anti-Patterns

### README-style documentation
❌ "We use Jest for unit testing. The test directory contains all test files organized by module."
✅ "1. MUST identify affected modules. 2. MUST run `npx jest --testPathPattern=\"<module>\"`. 3. If tests fail, MUST fix before proceeding."

### Commands without verification
❌ "1. Run `npm run build` 2. Run `npm run deploy`"
✅ "1. MUST run `npm run build`. If build fails, MUST read error output and fix. 2. MUST verify artifact: `ls dist/`. 3. MUST run `npm run deploy -- --env staging`. 4. MUST verify: `curl https://staging.example.com/health`."

### Monolithic skill
❌ One `development-workflow` skill covering testing, deployment, review, and migration.
✅ Separate focused skills: `run-tests`, `deploy-staging`, `code-review`, `generate-migration`.

### Decorative formatting
❌ Hard-wrapped paragraphs, `> Note:` boxes, emoji headers, three-deep bullet ladders.
✅ Flat numbered steps, tables for parallel data, code fences for commands.

### Missing RFC 2119/8174 keywords
❌ "Run the linter before committing"
✅ "MUST run the linter before committing"

❌ "Consider adding a test for edge cases"
✅ "SHOULD add a test for edge cases"

❌ "You can skip integration tests for internal refactoring"
✅ "MAY skip integration tests for internal-only refactoring"

## Structural Anti-Patterns

### Body over 500 lines without references/
Fix: MUST extract detailed reference material into `references/` files. SKILL.md MUST remain focused on the workflow.

### Critical constraints buried in the middle
Fix: MUST move non-negotiable rules to the top of the body under a `## Critical` or `## Rules` header.

### Ambiguous decision points
❌ "Process the file appropriately"
✅ "MUST run `python scripts/process.py input.pdf --format=markdown`"
