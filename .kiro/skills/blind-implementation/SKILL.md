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
| `{{impl_iteration}}` | The final main-loop iteration number (used in comment phase) |
| `{{comment_iteration}}` | Current comment-loop iteration number (integer) |
| `{{prev_comment_iteration}}` | `comment_iteration - 1` (integer, only used when comment_iteration > 1) |

## Procedure

1. Derive `<topic>` by slugifying the user's request (lowercase, hyphens, max 40 chars).
2. Check if `scratchpad/<topic>/` already exists.
   - If it exists, ask the user whether to clean it (delete contents) or resume from existing state.
   - If resuming, determine the last iteration number from existing versioned files.
3. Create directory `scratchpad/<topic>/` if it does not exist.
4. Write `scratchpad/<topic>/request.md` containing the user's full requirement verbatim.
5. Set `iteration = 1` (or resume value), `max_iterations = 3`.
6. Read `.kiro/skills/blind-implementation/references/planner-prompt.md`.
7. Read `.kiro/skills/blind-implementation/references/plan-reviewer-prompt.md`.
8. Read `.kiro/skills/blind-implementation/references/implementer-prompt.md`.
9. Read `.kiro/skills/blind-implementation/references/tester-prompt.md`.
10. Read `.kiro/skills/blind-implementation/references/reviewer-prompt.md`.
11. Read `.kiro/skills/blind-implementation/references/comment-fixer-prompt.md`.
12. Read `.kiro/skills/blind-implementation/references/comment-auditor-prompt.md`.
13. For each prompt template, replace all `{{variable}}` occurrences with actual values per the Template Variables table. Append a context block specifying the `request_file` path and, if iteration > 1, the `feedback_file` path.
14. Invoke the `subagent` tool:

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
      "name": "plan-reviewer",
      "role": "kiro_default",
      "prompt_template": "<plan_reviewer_prompt_with_substituted_variables>",
      "depends_on": ["planner"]
    },
    {
      "name": "implementer",
      "role": "kiro_default",
      "prompt_template": "<implementer_prompt_with_substituted_variables>",
      "depends_on": ["plan-reviewer"]
    },
    {
      "name": "tester",
      "role": "kiro_default",
      "prompt_template": "<tester_prompt_with_substituted_variables>",
      "depends_on": ["implementer"]
    },
    {
      "name": "reviewer",
      "role": "kiro_default",
      "prompt_template": "<reviewer_prompt_with_substituted_variables>",
      "depends_on": ["tester"]
    }
  ]
}
```

15. Verify `scratchpad/<topic>/review-v{iteration}.md` exists.
    - If the file does not exist, report the pipeline failure to the user and ask for guidance on how to proceed.
16. Read `scratchpad/<topic>/review-v{iteration}.md`. Parse the `verdict:` field from the YAML frontmatter.
    - If `verdict: PASS` → proceed to step 19.
    - If `verdict: FAIL` → continue to step 17.
17. Read the `## Issues` section from `scratchpad/<topic>/review-v{iteration}.md` and `scratchpad/<topic>/test-report-v{iteration}.md`. Write `scratchpad/<topic>/feedback-v{iteration}.md` containing both review issues and test failures with instruction to address them.
18. Increment `iteration`.
    - If `iteration <= max_iterations`: return to step 13.
    - If `iteration > max_iterations`: proceed to step 19 with a warning.

### Comment Refinement Phase

19. Set `impl_iteration = iteration` (the final main-loop iteration), `comment_iteration = 1`, `max_comment_iterations = 2`.
20. Substitute comment-phase variables (`{{impl_iteration}}`, `{{comment_iteration}}`, `{{prev_comment_iteration}}`) into the comment-fixer and comment-auditor prompt templates. If `comment_iteration` > 1, append the `comment_feedback_file` path.
21. Invoke the `subagent` tool:

```json
{
  "task": "Comment compliance review and audit",
  "mode": "blocking",
  "stages": [
    {
      "name": "comment-fixer",
      "role": "kiro_default",
      "prompt_template": "<comment_fixer_prompt_with_substituted_variables>"
    },
    {
      "name": "comment-auditor",
      "role": "kiro_default",
      "prompt_template": "<comment_auditor_prompt_with_substituted_variables>",
      "depends_on": ["comment-fixer"]
    }
  ]
}
```

22. Verify `scratchpad/<topic>/comment-audit-v{comment_iteration}.md` exists.
    - If the file does not exist, report the comment pipeline failure and skip to step 25.
23. Read `scratchpad/<topic>/comment-audit-v{comment_iteration}.md`. Parse the `verdict:` field.
    - If `verdict: PASS` → proceed to step 25.
    - If `verdict: NEEDS-ATTENTION` → continue to step 24.
24. Write `scratchpad/<topic>/comment-feedback-v{comment_iteration}.md` from the auditor's issues. Increment `comment_iteration`.
    - If `comment_iteration <= max_comment_iterations`: return to step 20.
    - If `comment_iteration > max_comment_iterations`: write remaining issues to `scratchpad/<topic>/comment-audit-remaining.md` and proceed to step 25.
25. Present to the user: final verdict, list of files created/modified, comment compliance summary, and any remaining comment issues.
26. Ask the user if they want to clean up `scratchpad/<topic>/`.

## File Convention

| File | Written By | Read By |
|------|-----------|---------|
| `request.md` | Main agent | Planner |
| `plan-v{N}.md` | Planner, Plan Reviewer (overwrite) | Plan Reviewer, Implementer, Reviewer |
| `plan-review-v{N}.md` | Plan Reviewer | Main agent (informational) |
| `impl-report-v{N}.md` | Implementer | Tester, Reviewer, Comment Fixer |
| `test-report-v{N}.md` | Tester | Reviewer, Main agent, Planner (next iteration) |
| `review-v{N}.md` | Reviewer | Main agent |
| `feedback-v{N}.md` | Main agent | Planner (next iteration) |
| `comment-fixes-report-v{N}.md` | Comment Fixer | Comment Auditor |
| `comment-audit-v{N}.md` | Comment Auditor | Main agent, Comment Fixer (next comment iteration) |
| `comment-feedback-v{N}.md` | Main agent | Comment Fixer (next comment iteration) |
| `comment-audit-remaining.md` | Main agent | Future invocations (informational) |
