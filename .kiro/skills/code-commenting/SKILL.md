---
name: code-commenting
description: "Use when writing, generating, or modifying code in any programming language. Also use when encountering unexpected behavior, API quirks, or counterintuitive semantics during implementation to document the discovery inline. Provides mandatory commenting guidelines covering when to comment, what to comment, algorithmic narration, language-specific doc conventions, and comment anti-patterns. Do NOT use for documentation files (README, wikis) or non-code text."
---

The keywords MUST, MUST NOT, REQUIRED, SHALL, SHALL NOT, SHOULD, SHOULD NOT, RECOMMENDED, NOT RECOMMENDED, MAY, and OPTIONAL in this document are to be interpreted as described in [RFC 2119](https://www.rfc-editor.org/rfc/rfc2119) and [RFC 8174](https://www.rfc-editor.org/rfc/rfc8174).

## Critical Constraints

- Comments MUST explain the reasoning (why) or the intent (what for), never merely restate syntax.
- Comments MUST be maintained alongside code changes — stale comments are worse than no comments.
- Commented-out code MUST NOT be committed. Use version control instead.
- Comments SHOULD be written in English unless the project explicitly mandates another language.
- In safety-critical systems requiring audit trails, or in explicitly pedagogical codebases, commenting the "what" MAY be appropriate when mandated by the domain's standards.

## Comment-Only Scope Rule

When the user request is specifically about code comments (adding, fixing, reviewing, or improving comments), the agent MUST NOT modify any non-comment code. Only comment lines (additions, modifications, deletions of comments) are permitted. Functional code, test assertions, variable declarations, control flow, imports, and any executable statement MUST remain untouched — even if the agent believes the code is incorrect or could be improved.

- If the agent identifies code that should change, it MUST flag it as a separate recommendation in its response text, not apply the change.
- This rule applies when the user's intent is comment-focused (e.g., "add comments", "fix comments", "review commenting", "make comments compliant"). It does NOT apply when the user asks for a general code change that incidentally requires comment updates.

## Comment-Code Alignment Invariant

Comments MUST always describe what the code **actually does**, never what it is assumed or intended to do. A comment describing desired behavior while the code implements different behavior is a documentation defect that actively misleads readers.

### During comment review sessions

When reviewing or writing comments, if the agent detects a misalignment between what the code does and what it should do from a functional or non-functional perspective:

1. The comment MUST describe the code's **actual behavior** accurately.
2. A separate `TODO` comment MUST be appended immediately after, flagging the suspected misalignment for review in a subsequent session.
3. The TODO MUST include:
   - The keyword `POSSIBLE-BUG` to distinguish from regular TODOs
   - A description of expected behavior vs. actual behavior
   - Why the agent believes there is a misalignment
4. The agent MUST NOT fix the code — only document the discrepancy (per the Comment-Only Scope Rule).

### Format

```
// <accurate description of what the code actually does>
// TODO(reviewer): POSSIBLE-BUG — <expected behavior> vs <actual behavior>; <reasoning> [needs-review]
```

### Example

```rust
// Returns early when the list is empty, skipping validation entirely
// TODO(reviewer): POSSIBLE-BUG — empty lists likely still need schema validation
//   per the API contract; skipping may violate the POST /items spec [needs-review]
if items.is_empty() {
    return Ok(());
}
```

### Scope of misalignment detection

Misalignment includes but is not limited to:
- Functional: wrong computation, incorrect boundary condition, missing case handling
- Non-functional: missing input validation, unsafe error suppression, resource leaks, race conditions
- Contractual: behavior contradicts documented API contract or violates stated invariants

### Confidence qualifier

When the agent is uncertain whether the code is actually wrong (vs. intentionally non-obvious), the TODO MUST use `POSSIBLE-BUG` rather than asserting a definitive defect. The purpose is to flag for human review, not to declare a verdict.

## When to Comment

Comment in these situations:

| Situation | Guidance |
|-----------|----------|
| Control flow | Every conditional, loop, and iterator chain MUST be commented with its intent (see "Control Flow Commenting" section) |
| Business logic and intent | Explain the business rule or product decision behind a code block |
| Non-obvious trade-offs | Document why a particular approach was chosen over alternatives |
| Workarounds and hacks | Mark temporary fixes with context and link to the tracking issue |
| Regulatory or compliance | Flag code that exists due to legal, security, or compliance requirements |
| Complex algorithms | Narrate the algorithm's progression per the "Algorithmic Narration" section below |
| Warnings and gotchas | Alert about non-obvious side effects, ordering dependencies, or concurrency concerns |
| Tricky or kludgy code | Per the "Surprise and Confusion Documentation" section below |
| Public API boundaries | Document parameters, return values, exceptions, and usage contracts |

## Algorithmic Narration

For non-trivial algorithmic functions (~30+ lines of dense logic, multiple phases, or complex state transformations):

- Inline comments MAY narrate the algorithm's progression — describing what each phase accomplishes, what invariants hold, and how intermediate state relates to the final result.
- A high-level summary comment at the function top SHOULD describe the overall approach, reference any paper/RFC/algorithm name, and outline the phases.
- Phase-boundary comments SHOULD mark transitions between logical steps (e.g., `// Phase 2: merge adjacent siblings bottom-up`).
- Invariant comments SHOULD state what is guaranteed to be true at key points when this aids comprehension.
- This is NOT a license to paraphrase trivial lines. Narration applies to the algorithmic flow, not to individual simple statements within it.

## Surprise and Confusion Documentation

Code that is misunderstanding-prone, tricky, or kludgy MUST be commented. This is mandatory, not discretionary.

### Mandatory triggers — comment MUST be added when:

| Trigger | Example |
|---------|---------|
| Code relies on subtle language/runtime behavior | Integer overflow wrapping, floating-point comparison, implicit coercion |
| Code looks wrong but is intentional | Off-by-one that is correct due to boundary semantics |
| Order-dependent operations where reordering looks safe | Initialization sequences, lock acquisition order |
| API used in non-obvious way due to quirk or undocumented behavior | Calling a method with unusual parameters to trigger a side effect |
| Bit manipulation or arithmetic tricks | Mask operations, power-of-two tricks, hash combining |
| Performance optimization that sacrifices readability | Loop unrolling, cache-line alignment, branch prediction hints |
| Workaround for a bug in a dependency or platform | With version/issue reference |
| Code that contradicts common patterns in the same codebase | Different approach needed here due to specific constraint |

### Agent-specific activation rule

When the agent discovers something unexpected during implementation — a compiler error revealing unexpected semantics, an API that doesn't behave as assumed, a subtle edge case, or a behavior that contradicts initial assumptions — the agent MUST:

1. Add an inline comment at the relevant code location explaining the unexpected behavior.
2. State what the intuitive expectation was and why reality differs.
3. Include a reference (doc link, issue, error message) when available.

Rationale: if it surprised the agent, it will surprise the next human reader.

## Control Flow Commenting

Every conditional (`if`, `else`, `match`, `switch`), loop (`for`, `while`, `loop`), and iterator chain (`.filter()`, `.map()`, `.fold()`, etc.) MUST have a comment explaining its purpose or the condition being tested. This applies regardless of perceived simplicity — control flow comments document execution flow and aid rapid code comprehension.

- The comment MUST describe the intent or business meaning of the branch/iteration, not merely restate the syntax.
- For chained iterators, a single comment above the chain describing the overall transformation is sufficient.
- For `else` branches, a comment is REQUIRED when the else-case is non-obvious; RECOMMENDED in all cases.

## When NOT to Comment

- MUST NOT use comments as a substitute for meaningful variable/function names. Rename instead.
- MUST NOT write journal comments (author, date, changelog). Use git history.
- MUST NOT use banner/separator comments to organize code. Use functions and modules instead.
- MUST NOT leave TODO/FIXME without a tracking reference (issue ID or link).
- License and copyright headers REQUIRED by the project's licensing policy are exempt from these rules.

## Module-Level Comments

- Every file SHOULD have a brief module-level comment stating its purpose when the filename alone is insufficient.
- Module comments MUST describe the responsibility of the module, not enumerate its contents.
- Module comments SHOULD mention key dependencies or architectural context when non-obvious.
- If the module is part of a larger subsystem, SHOULD reference the parent component or design doc.

## Documentation Comments (Public APIs)

For public/exported interfaces, MUST use the language-idiomatic doc format:

| Language | Format | Tool |
|----------|--------|------|
| Python | `"""docstring"""` (Google/NumPy style) | Sphinx |
| JavaScript/TypeScript | `/** JSDoc */` | TypeDoc/JSDoc |
| Java/Kotlin | `/** Javadoc */` | Javadoc |
| Rust | `///` or `//!` | rustdoc |
| Go | `// FuncName ...` preceding declaration | godoc |
| C/C++ | `/** Doxygen */` or `///` | Doxygen |
| C# | `/// <summary>` XML comments | DocFX |
| Ruby | `# YARD` comments | YARD |
| Swift | `///` doc comments | DocC |
| PHP | `/** PHPDoc */` | phpDocumentor |

Documentation comments MUST include:
1. A concise one-line summary of purpose.
2. Parameter descriptions when names alone are insufficient.
3. Return value semantics when non-obvious.
4. Exceptions/errors that callers must handle.
5. Usage example — RECOMMENDED for complex APIs.

Documentation comments MUST NOT include:
- Implementation details that may change.
- Redundant type information already expressed by the type system.

## Language-Specific Documentation Conventions

When the target language has well-established documentation-from-code conventions, the generated documentation comments MUST follow the idiomatic conventions of that ecosystem. This section is mandatory and overrides any generic formatting instinct.

### Canonical tag/section ordering

Documentation tags and sections MUST follow the language's standard ordering:

| Language | Required ordering |
|----------|-------------------|
| Java/Kotlin | `@param` → `@return` → `@throws` → `@see` → `@since` → `@deprecated` |
| Python (Google) | Summary → `Args:` → `Returns:` → `Raises:` → `Note:` → `Example:` |
| Python (NumPy) | Summary → `Parameters` → `Returns` → `Raises` → `Notes` → `Examples` |
| Python (Sphinx) | Summary → `:param:` → `:returns:` → `:raises:` → `:example:` |
| Rust | Summary → blank line → details → `# Examples` → `# Errors` → `# Panics` → `# Safety` |
| Go | First sentence (starts with function name) → details → no tags |
| C# | `<summary>` → `<typeparam>` → `<param>` → `<returns>` → `<exception>` → `<remarks>` → `<example>` |
| Swift | Summary → `- Parameters:` → `- Returns:` → `- Throws:` → `- Note:` |
| PHP | Summary → `@param` → `@return` → `@throws` → `@see` → `@deprecated` |
| Ruby (YARD) | Summary → `@param` → `@return` → `@raise` → `@example` → `@see` |

### Summary sentence rules

The first sentence of a documentation comment MUST follow language-specific conventions:

- **Java/Kotlin**: Third-person declarative verb form ("Gets the user name", not "Get the user name").
- **Go**: MUST start with the declared name ("FuncName does X"). Package doc goes in `doc.go`.
- **Rust**: Single-line `///` summary, then blank `///` line before extended description.
- **Python**: Imperative mood ("Return the user name", not "Returns the user name") per PEP 257.
- **Swift**: Third-person declarative ("Returns the user name").
- **C#**: Third-person declarative within `<summary>` tags.

### Testable documentation examples

When the language supports executable doc-examples, complex public APIs SHOULD include a runnable example:

| Language | Mechanism | Requirement |
|----------|-----------|-------------|
| Rust | `/// # Examples` with code fence | SHOULD compile and pass as doctest |
| Python | `>>> ` lines in docstring | SHOULD pass `doctest` or `pytest --doctest-modules` |
| Go | `func ExampleFuncName()` in `_test.go` | SHOULD compile and match `// Output:` comment |
| Elixir | `iex>` lines in `@doc` | SHOULD pass `mix test` doctests |
| Java | `{@snippet}` (Java 18+) or `<pre>` block | RECOMMENDED to be compilable |

### Semantic sections

MUST use the language's standard sections for error conditions, safety, and lifecycle:

- **Rust**: `# Errors` (when returning `Result`), `# Panics` (when function can panic), `# Safety` (for `unsafe` functions — REQUIRED).
- **Python**: `Raises:` section listing each exception and condition. `Warning:` or `Note:` for important caveats.
- **Java**: `@throws`/`@exception` for each checked exception. `@implNote` for implementation notes.
- **Swift**: `- Precondition:`, `- Postcondition:`, `- Invariant:` for contract documentation.
- **C#**: `<exception cref="T">` for each thrown exception. `<remarks>` for extended discussion.

### Project style consistency

- If the project already uses a specific docstring style variant (Google vs. NumPy vs. Sphinx reST for Python, `/** */` vs. `///` for C++), MUST follow the established project convention.
- Detect project convention by examining existing documented files before writing new documentation.

### Linter/generator compatibility

- Documentation MUST pass the project's configured doc linter without warnings when one exists.
- Common linters: `clippy::missing_docs` (Rust), `pydocstyle` (Python), `checkstyle JavadocMethod` (Java), `eslint jsdoc/*` (JS/TS), `golint` (Go), `SA1600` (C# StyleCop).

### Fallback rule

When unsure of the idiomatic doc convention for a language, the agent MUST look it up via documentation tools or Context7 rather than guessing. Applying conventions from one language to another (e.g., `@param` tags in a Python docstring) is a defect.

## Inline Comment Style

- MUST be concise — one line preferred, two lines maximum for standard comments. Algorithmic narration and surprise documentation MAY use three lines when necessary for clarity.
- SHOULD be placed on the line above the code they describe. MAY be placed as a trailing comment when the comment is under 30 characters and the total line length remains within the project limit.
- MUST use a single space after the comment delimiter (`// like this`, `# like this`).
- SHOULD use sentence case without a trailing period for single-line comments.
- MUST match the indentation level of the code they annotate.

## TODO/FIXME Convention

- RECOMMENDED format: `// TODO(owner): description [ISSUE-123]`
- MUST include an owner (person or team alias).
- MUST include a tracking reference (issue, ticket, or link).
- SHOULD include a target date or milestone for resolution.
- When a project has an established TODO convention that satisfies the owner and tracking reference requirements, follow the project convention instead.

## Test Code Commenting

Test functions REQUIRE comments that document what is being tested. These comments serve a different purpose than production code comments — they establish trust, enable rapid failure diagnosis, and communicate the tester's mental model.

- Each test function SHOULD have a brief doc-comment stating the scenario or invariant being verified.
- **Setup comments** MUST document the test scenario when numeric literals, domain-specific constants, or constructed inputs require context to understand. Examples: `// Account with balance exactly at overdraft threshold`, `// Request payload exceeding the 4KB size limit`, `// Two adjacent intervals that should merge into one`.
- **Assertion comments** SHOULD explain what each assertion verifies when the assertion expression alone is not self-documenting. This is especially important when:
  - The expected value requires domain knowledge to derive (e.g., computed sizes, hash outputs, protocol codes)
  - Multiple assertions test different facets of the same operation (e.g., "exact match", "partial overlap", "no overlap", "boundary case")
  - The assertion validates a non-obvious invariant (e.g., "deduplication reduces count to 1", "order is preserved after sort")
- **Boundary/edge-case comments** MUST label which boundary condition each assertion exercises.
- The "When NOT to Comment" anti-patterns still apply, but test scenario documentation is NOT syntax restatement — it provides the mental model needed to understand why a specific value is expected.
- Removing test comments during a comment-compliance pass is PROHIBITED unless the comment is factually wrong or genuinely restates the assertion syntax (e.g., `// assert true` above `assert!(result)`).

## AI-Generated Code

When generating code, apply all preceding rules without exception.
- MUST comment every conditional, loop, and iterator chain per the "Control Flow Commenting" section.
- SHOULD add a brief top-level comment explaining the overall approach when generating complex solutions.
- MUST apply the "Surprise and Confusion Documentation" rules when encountering unexpected behavior during implementation.

## Verification

After writing or modifying code with comments:
1. Verify every conditional, loop, and iterator chain in modified code has an intent comment.
2. For each comment in modified code, verify it provides information not expressed by the code itself — unless it falls under algorithmic narration, control flow, or surprise documentation.
3. Verify documentation comments match the current function signature and behavior.
4. Verify all public/exported interfaces have documentation comments per the "Documentation Comments" section.
5. Verify documentation comments follow the language-specific conventions per the "Language-Specific Documentation Conventions" section (tag ordering, summary sentence style, semantic sections).
6. Scan comments within 10 lines of any modified code for staleness or inaccuracy.
7. Verify that tricky, kludgy, or counterintuitive code has an explanatory comment.
