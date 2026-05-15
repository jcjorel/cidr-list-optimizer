---
name: blind-implementation
description: Use when performing a blind implementation, large feature build, multi-file coding task, or full module creation where context preservation is critical. Handles iterative planning, implementation, and adversarial review via subagents in a loop. Do NOT use for small edits, single-file fixes, refactoring, or questions.
---

The keywords MUST, MUST NOT, REQUIRED, SHALL, SHALL NOT, SHOULD, SHOULD NOT, RECOMMENDED, NOT RECOMMENDED, MAY, and OPTIONAL in this document are to be interpreted as described in [RFC 2119](https://www.rfc-editor.org/rfc/rfc2119) and [RFC 8174](https://www.rfc-editor.org/rfc/rfc8174).

## Constraints

- The main agent MUST NOT perform planning, implementation, or review directly.
- The main agent MUST delegate all heavy work to subagents to preserve its context window.
- All inter-agent communication MUST use files in `scratchpad/<topic>/`.
- The orchestration loop MUST NOT exceed 3 iterations unless the user explicitly requests more.
- Each subagent stage MUST use `role: "kiro_default"` with behavior injected via `prompt_template`.
- Artifacts MUST be versioned across iterations (`plan-v1.md`, `plan-v2.md`, etc.).
- All file paths in subagent prompts MUST be relative to the project root directory.

## Template Variables

When constructing prompt templates, replace these variables with actual values:

| Variable | Replacement |
|----------|-------------|
| `{{topic}}` | `scratchpad/<topic>` (the full relative path) |
| `{{iteration}}` | Current iteration number (integer) |
| `{{prev_iteration}}` | `iteration - 1` (integer, only used when iteration > 1) |

## Procedure

1. Derive `<topic>` by slugifying the user's request (lowercase, hyphens, max 40 chars).
2. Check if `scratchpad/<topic>/` already exists.
   - If it exists, ask the user whether to clean it (delete contents) or resume from existing state.
   - If resuming, determine the last iteration number from existing versioned files.
3. Create directory `scratchpad/<topic>/` if it does not exist.
4. Write `scratchpad/<topic>/request.md` containing the user's full requirement verbatim.
5. Set `iteration = 1` (or resume value), `max_iterations = 3`.
6. Read `.kiro/skills/blind-implementation/references/planner-prompt.md`.
7. Read `.kiro/skills/blind-implementation/references/implementer-prompt.md`.
8. Read `.kiro/skills/blind-implementation/references/reviewer-prompt.md`.
9. For each prompt template, replace all `{{variable}}` occurrences with actual values per the Template Variables table. Append a context block specifying the `request_file` path and, if iteration > 1, the `feedback_file` path.
10. Invoke the `subagent` tool:

```json
{
  "task": "<user's request summary>",
  "mode": "blocking",
  "stages": [
    {
      "name": "planner",
      "role": "kiro_default",
      "prompt_template": "<planner_prompt_with_substituted_variables>"
    },
    {
      "name": "implementer",
      "role": "kiro_default",
      "prompt_template": "<implementer_prompt_with_substituted_variables>",
      "depends_on": ["planner"]
    },
    {
      "name": "reviewer",
      "role": "kiro_default",
      "prompt_template": "<reviewer_prompt_with_substituted_variables>",
      "depends_on": ["implementer"]
    }
  ]
}
```

11. Verify `scratchpad/<topic>/review-v{iteration}.md` exists.
    - If the file does not exist, report the pipeline failure to the user and ask for guidance on how to proceed.
12. Read `scratchpad/<topic>/review-v{iteration}.md`. Parse the `verdict:` field from the YAML frontmatter.
    - If `verdict: PASS` → proceed to step 15.
    - If `verdict: FAIL` → continue to step 13.
13. Read the `## Issues` section from `scratchpad/<topic>/review-v{iteration}.md`. Write `scratchpad/<topic>/feedback-v{iteration}.md` containing the issues and instruction to address them.
14. Increment `iteration`.
    - If `iteration <= max_iterations`: return to step 9.
    - If `iteration > max_iterations`: proceed to step 15 with a warning.
15. Present to the user: final verdict, list of files created/modified, and any remaining issues if max iterations was reached.
16. Ask the user if they want to clean up `scratchpad/<topic>/`.

## File Convention

| File | Written By | Read By |
|------|-----------|---------|
| `request.md` | Main agent | Planner |
| `plan-v{N}.md` | Planner | Implementer, Reviewer |
| `impl-report-v{N}.md` | Implementer | Reviewer |
| `review-v{N}.md` | Reviewer | Main agent |
| `feedback-v{N}.md` | Main agent | Planner (next iteration) |
