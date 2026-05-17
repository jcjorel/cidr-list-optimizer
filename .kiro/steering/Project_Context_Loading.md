---
name: project-context-loading
description: Defines mandatory project file loading at session start. Use when starting a new session that refers to the current project.
---

# Project Context Loading

The key words "MUST", "MUST NOT", "SHOULD", "SHOULD NOT", and "MAY" in this document are to be interpreted as described in [RFC 2119](https://www.rfc-editor.org/rfc/rfc2119) and [RFC 8174](https://www.rfc-editor.org/rfc/rfc8174).

## Rules

1. **At new session starts**, when an initial user question or request refers to the current project, the agent MUST read the following files before responding:
   - `README.md`
   - `docs/ARCHITECTURE.md`
   - `docs/USER_GUIDE.md`
   - `docs/GETTING_STARTED.md`
   - `docs/DEVELOPER_API.md`

2. The skill `project-documentation-mandatory-guidelines` MUST NOT be loaded unless the question or request explicitly concerns reading, writing, creating, improving, or reviewing project documentation files.
