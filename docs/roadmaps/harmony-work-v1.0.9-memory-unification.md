# Harmony Work 1.0.9: memory and skill lifecycle

Status: in progress. This document is the acceptance ledger for a testable Windows build; a full bootstrapper installer is explicitly deferred until user testing passes.

## Product decisions

- Harmony Work's local SQLite `memories` table is the only writable source of truth for Xiaoheng's long-term memory. Codex Memory concepts and self-improvement skills access it through the WorkBuddy MCP or application commands, not independent JSONL stores.
- Embeddings and full-text indexes are derived data. Memory writes must succeed without the embedding service; retrieval falls back to lexical search when embeddings are unavailable.
- Personal cloud sync transfers encrypted logical memory records, not `workbuddy.db` or a third-party memory directory. Deletions require tombstones so another device cannot resurrect them.
- A full local uninstall through the standalone uninstaller removes application-owned installed skills and user data, but preserves the shared runtime at `%LOCALAPPDATA%\Harmony\WorkBuddy\runtime`. It must not remove the shared market, unrelated Codex skills, user workspaces, or cloud records. A subsequent login may restore cloud records only after an explicit sync choice.
- The selected workspace is always retained, even when it is nested under an application-data root. A full-data uninstall writes only a migration-block marker inside the retained runtime, so legacy app data cannot reappear on reinstall.
- The author's Codex-compatible `self-improvement` skill is packaged for the Harmony market. Packaging alone does not guarantee automatic activation; the Codex runtime integration and skill invocation must be verified separately.

## Acceptance checklist

### 1. Data ownership and migration

- [x] Stop legacy `com.piscis.desktop` data from re-importing after a migration or explicit data clear.
- [ ] Ensure UI, scheduled consolidation, and MCP memory tools read/write the same SQLite records and support Chinese content.
- [ ] Preserve existing user memory during upgrade; do not import deleted legacy records again.
- [ ] Ensure deleting one or all memories also prevents cloud or legacy resurrection unless the user explicitly restores them.

### 2. Memory creation and retrieval

- [ ] Scheduled and manual consolidation call tools that actually exist in the Codex runtime; confirm a run writes a durable record visible in the memory page.
- [ ] Add a configurable Harmony embedding model, validate its endpoint, and index new/changed memories without blocking durable writes.
- [ ] Implement owner-scoped hybrid retrieval and a lexical fallback; verify Chinese queries and model-change reindexing.
- [ ] Provide explicit status/errors for indexing and consolidation rather than reporting success when no record was written.

### 3. Self-improvement skill

- [ ] Obtain the original author's Codex-compatible upstream source and preserve attribution/license.
- [x] Adapt its storage instructions to the unified WorkBuddy memory tools without a second writable `.learnings` store.
- [x] Package/validate `<id>.skill.zip` under `/Volumes/Harmony共享/harmony-skill` using the Harmony skill manifest contract (market API lists version 1.0.1).
- [ ] Verify installation, discovery, invocation, and uninstall in the packaged Windows runtime.

### 4. Personal cloud sync

- [ ] Add a separate selectable memory category without silently changing existing settings categories.
- [ ] Encrypt records with the existing account data key; use stable IDs, revisions, and tombstones for deterministic merges.
- [ ] Keep device-local embeddings out of the cloud payload and rebuild them after restore.
- [ ] Cover backup, restore, delete, two-device conflicts, offline changes, and account isolation with tests.

### 5. Full uninstall and test build

- [x] Add an explicit full-data-removal choice in the standalone uninstaller, including installed skill directories and app-owned caches. The unsafe built-in NSIS full-data checkbox is blocked before deletion; use the standalone tool for deep cleanup.
- [x] Keep ordinary uninstall data-preserving and never delete shared skill packages or user workspaces (policy tests passed on Windows).
- [x] Add a branded standalone uninstaller that invokes the registered main uninstaller, then scans only known Harmony Work residual locations for optional user-reviewed cleanup; retain the runtime (Windows build and policy tests passed; real uninstall pending user test).
- [x] Bump all application version sources to `1.0.9`.
- [x] Run focused tests, frontend build, Rust checks, and Windows build on the remote E-drive checkout.
- [ ] Report exact test-build path and unresolved manual checks. Do not produce the full bootstrapper until user approval after testing.

## Known failure paths to close

- `memory_consolidation` currently instructs the agent to call `memory_store`, while the WorkBuddy MCP exposes `memory_search` and `memory_append`.
- `migrate_legacy_app_data` currently runs on startup whenever the old directory exists.
- Account settings cloud snapshots contain settings categories only; they have no memory records or deletion semantics.
- A Harmony market `.skill.zip` supplies a skill, not trusted Codex lifecycle hooks.

## Work log

- 2026-10-10: Confirmed current `main` is clean at `872d67d`; created `feat/v1.0.9-memory-unification` on the remote E-drive checkout.
- 2026-10-10: Added the unified MCP memory path, optional Harmony embeddings with lexical fallback, encrypted cloud memory changes/tombstones, and Codex-compatible self-improvement skill package. Windows Rust library tests: 237 passed; market/runtime end-to-end checks remain open.
- 2026-10-10: Added branded WPF standalone uninstaller. Windows cleanup policy assertions passed; release test executable is built without producing the full bootstrapper.
- 2026-10-10: Market API confirms `self-improvement-codex` 1.0.1 with only `mcp:workbuddy` permission; live skill invocation remains unverified.
- 2026-10-11: The first NSIS 1.0.9 test build completed. Added regression tests for cloud re-observation revisions and fail-closed removal of an unknown workspace in the legacy data root. Rebuild of the final test package is pending.
- 2026-10-11: User testing exposed cross-product history after clean install. Root cause: startup copied the previous product's app-data tree and database, MCP independently imported old memories, schema migration reassigned old memory ownership, and journal copied old undo history. Those automatic cross-product imports are removed. Existing Harmony `workbuddy.db` is preserved because its genuine and imported records cannot be distinguished safely. To validate a truly clean state, run the standalone deep uninstaller (retaining workspace/runtime), then install the new test package without restoring cloud data.
- 2026-10-11: Scheduled memory consolidation now limits input to the past 30 days, excludes scheduled sessions, and includes only task spines associated with included sessions. API settings now fetch Harmony models for the configured key, classify embedding models, display them in a dropdown, and preselect the first available model only when no valid selection exists. Static, Rust, MCP, frontend, and build verification are in progress for the rebuilt test package.
- 2026-10-11: Rebuilt test installer at `E:\harmony-work\release\test\Harmony-Work-1.0.9-x64-test-setup.exe` (SHA-256 `C58D1983173B3AA0B6C1FF5CB64CBC7F26A1DA325E0B11945DC6BFDA75357EE5`). Standalone uninstaller at `E:\harmony-work\release\test\Harmony-Work-1.0.9-x64-uninstaller.exe` (SHA-256 `CB3C4267E69F3B35F8267ADE788AAC68D91B1A46B2FEDB03AFF228A54BD4EF5B`). Verification: frontend 188 tests, Node contracts 33 tests, Rust desktop 238 tests, MCP host 32 tests, Web build, Windows NSIS release build, uninstaller policy assertions all passed. Real install/uninstall and authenticated Harmony model-catalog selection still require manual acceptance on a test device.
