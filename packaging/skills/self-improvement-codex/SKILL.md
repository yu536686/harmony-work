---
name: self-improvement-codex
description: Capture verified user corrections, recurring failures, and durable workflow improvements in Harmony Work's unified Xiaoheng memory via WorkBuddy MCP.
dependencies:
  - mcp:workbuddy
permissions:
  - mcp:workbuddy
---

# Self-Improvement for Harmony Work

This Codex adaptation is inspired by pskoett's multi-agent self-improvement workflow:
https://github.com/pskoett/pskoett-ai-skills/tree/main/skills/self-improvement
The storage and invocation protocol below is Harmony Work-specific. It does not copy or install the upstream hook scripts.

## When to use

- The user corrects a persistent preference, fact, or process.
- An unexpected failure is diagnosed, fixed, and verified; the lesson will prevent recurrence.
- A workflow improvement proves useful across more than one task.
- The user explicitly asks Xiaoheng to remember a durable lesson.

Do not save raw transcripts, secrets, credentials, one-off errors, speculative fixes, or routine task progress. A current failure should be resolved and tested before its lesson is stored.

## Capture protocol

1. State the durable lesson in one concise sentence, including the affected scope and the verified action.
2. Call WorkBuddy MCP `memory_search` with distinctive terms and `owner_id: "xiaoheng"`.
3. If an existing record already says the same thing, do not append another. If the old record is wrong, ask for a correction path instead of accumulating contradictory memories.
4. Call WorkBuddy MCP `memory_append` only when the lesson is new and verified. Use `category: "preference"` for user preferences, `"error_learned"` for fixed failures, or `"decision"` for enduring decisions.
5. Report that a memory was saved only after the tool returned a real memory ID. If the tool failed, say so plainly.

The only writable long-term memory is Harmony Work's local `memories` database through WorkBuddy MCP. Do not create `.learnings`, `MEMORY.md`, JSONL, or a second history store for this skill. The Harmony Work memory page reads the same records; cloud sync, when selected by the user, transfers encrypted logical records and deletion markers.

## Review protocol

Before a task in an area with previous corrections, call `memory_search` for that area. Verify time-sensitive facts again rather than treating stored memories as permanent truth. During scheduled consolidation, keep only the few durable items that survive this check.

This package does not register Codex plugin hooks. In Harmony Work, the desktop runtime supplies a lightweight opt-in reminder when this installed skill is enabled; the WorkBuddy MCP performs durable capture and search.
