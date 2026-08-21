# Harness 0.9 Runtime

## Goal

Reduce avoidable model turns while keeping tool execution, approvals, progress and failures inspectable.

## Phase 1: one model-driven execution entry

- Main chat no longer makes a separate intent-router model request before the agent run.
- The first main-model turn receives the enabled tool schemas and chooses whether to answer, inspect, execute, delegate or plan.
- The old route-dependent iteration cap is replaced by one bounded `4..=24` run budget.

## Phase 2: progress without hidden reasoning

- The runtime emits `run_status` events for `thinking`, `executing_tools` and `retrying`.
- The chat activity panel renders these events independently from assistant text and tool receipts.
- Tool start/end events remain the durable evidence for what actually ran.

## Phase 3: repeat and plan discipline

- `plan_todo` canonicalises whitespace and treats an unchanged board as a no-op.
- Repeated plan projections warn at 3, 5 and 8 attempts but are never promoted to a global run stop.
- Identical operational tool calls require eight unchanged results before their own call is rejected.
- Rejected calls are scoped to the specific call. Other calls in the same turn remain available for recovery.

## Verification targets

- Rust kernel tests prove unchanged plans do not emit a second plan update and plan repeats do not become a hard circuit breaker.
- Frontend unit tests and production build compile the typed `run_status` event path.
- Desktop packaging uses version `0.9.0` consistently in npm, Cargo and Tauri metadata.
