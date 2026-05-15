---
name: skill-writer
description: USE when creating, reviewing, modifying, improving and any Kiro skills (SKILL.md files) related tasks to get mandatory guidance. Handles skill structure, description optimization, body formatting, and progressive disclosure. Do NOT use for general documentation writing.
---

# Skill Writer

## Workflow — Creating a New Skill

1. Ask the user what task the skill MUST automate. Identify: trigger conditions (when), capabilities (what), and target audience (model-invoked, user-invoked, or both).
2. Choose a name: MUST be lowercase, hyphens only, 1-64 chars. MUST match the parent directory name. SHOULD use verb-first for actions (`generate-migration`) or gerund for processes (`code-reviewing`).
3. Write the description (MUST be 1-1024 chars):
   - MUST state *when* to activate and *what capabilities* exist — MUST NOT reveal the procedure.
   - MUST include concrete keywords users would type.
   - SHOULD add negative triggers if the scope risks over-triggering.
   - MUST use third person, declarative voice: "Use when…", "Handles…". MUST NOT use "I can help you…".
4. Write the body following the formatting rules below.
5. If body exceeds ~300 lines, MUST extract reference material into `references/`.
6. Validate: name MUST match directory name, description MUST be ≤1024 chars, body MUST be procedural.

## Body Formatting Rules

- MUST use imperative form: "Run the linter" not "You should run the linter".
- MUST use flat numbered steps with concrete commands. MUST NOT use three-deep bullet ladders.
- SHOULD use tables for parallel structured data (decision matrices, status codes, file mappings).
- MUST use code fences with accurate language tags for all commands and code.
- MUST NOT use hard-wrapped prose, blockquotes, "Note:" boxes, or decorative dividers.
- MUST NOT include explanations the model already knows (e.g., don't explain what git is).
- SHOULD use examples as input/output pairs when format clarity is needed.
- SHOULD use conditional logic with `- If … :` branches when workflows diverge.
- MUST include verification steps after every action that could fail.
- MUST place critical constraints at the TOP of the body, not buried in the middle.
- Sequential procedures MUST use RFC 2119 / RFC 8174 keywords (MUST, MUST NOT, SHOULD, SHOULD NOT, MAY, REQUIRED, SHALL, SHALL NOT, RECOMMENDED, OPTIONAL) to express obligation levels. Keywords MUST appear in UPPERCASE when used with their normative meaning.
- Every skill body MUST begin with the following RFC 2119 / RFC 8174 preamble as its first paragraph (before any heading or procedure):

The keywords MUST, MUST NOT, REQUIRED, SHALL, SHALL NOT, SHOULD, SHOULD NOT, RECOMMENDED, NOT RECOMMENDED, MAY, and OPTIONAL in this document are to be interpreted as described in [RFC 2119](https://www.rfc-editor.org/rfc/rfc2119) and [RFC 8174](https://www.rfc-editor.org/rfc/rfc8174).

## Directory Structure

```
skill-name/
├── SKILL.md          # REQUIRED: frontmatter + instructions (<500 lines)
├── references/       # OPTIONAL: detailed docs loaded on demand
├── scripts/          # OPTIONAL: executable code
└── assets/           # OPTIONAL: templates, schemas
```

## Description Quality Checklist

| Check | Pass criteria |
|-------|--------------|
| Trigger, not summary | MUST NOT reveal the step-by-step procedure |
| Specific keywords | MUST contain terms users actually type |
| Negative triggers | SHOULD exclude adjacent use cases if scope is broad |
| Length | MUST be 2-4 sentences, ≤1024 chars |
| Voice | MUST be third person, declarative |
| Imperative test | MUST complete: "Activate this skill when the user…" |

## Workflow — Reviewing an Existing Skill

1. Read the SKILL.md frontmatter. MUST check name format and description against the checklist above.
2. Read the body. MUST flag violations:
   - Missing RFC 2119/8174 preamble at the start of the body
   - Documentation prose instead of procedures
   - Description leaking the workflow
   - Missing verification steps
   - Decorative formatting (blockquotes, emoji headers, nested bullets)
   - Body over 500 lines without `references/` extraction
   - Ambiguous language at decision points
   - Missing RFC 2119/8174 keywords in sequential procedures
3. MUST suggest concrete rewrites for each violation found.

## Kiro-Specific Notes

- Workspace skills: `.kiro/skills/<name>/SKILL.md`
- Global skills: `~/.kiro/skills/<name>/SKILL.md`
- Workspace skills override global skills with the same name.
- MAY invoke manually with `/<name>` or let Kiro auto-activate from description match.
- SHOULD use `/context show` to verify a skill is loaded.

## Token Budget Guidelines

| Component | Target |
|-----------|--------|
| Description | MUST be ≤1024 chars (~150 tokens) |
| Body (frequently activated) | SHOULD be <200 lines (~300 tokens) |
| Body (standard) | SHOULD be <500 lines (~2000-3000 tokens) |
| Reference files | MAY be unlimited (loaded on demand) |
