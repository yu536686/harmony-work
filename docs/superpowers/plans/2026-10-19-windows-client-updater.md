# Windows 1.1.1 / 1.1.2-rc.1 updater implementation plan

User approved development on a new branch, CDN publication of installers, and adding the portal light download. Versions are explicitly confirmed as stable 1.1.1 and preview 1.1.2-rc.1. Existing source changes form the 1.1.0 baseline and must be preserved. No main checkout mutation.

## Work areas and ownership

- Desktop clone: `/Users/yutianqi/Documents/小亨workbuddy/build/harmony-work-updater`, branch `feat/windows-light-updater-20261019`, baseline commit `767d90d`.
- Backend agent owns `src-tauri/src/updater/**`, Rust module registration, bootstrap and Rust dependency/config changes needed for updater. Root owns `package.json` version, frontend UI/store/services, shared release signing config and integration.
- Installer agent owns `installer/light/**`, `scripts/build-light-installer*`, payload/compatibility manifest generation and related tests; do not edit backend or frontend.
- Portal agent owns an isolated portal worktree and portal tests. Root deploys after integration.

## Global constraints

Use HTTPS manifests and actual installer download links under `https://download.harmonywatch.cn/`. Stable and preview channels are separate; user can enable preview in About for 1.1.1 -> 1.1.2-rc.1 testing. Full stable installer retains complete offline first-install support. Light package overlays core files and preserves all installed compatible resources, user data, workspace and uninstall capabilities. Reject incompatible/missing dependencies with a clear error before modifying installed files. Do not silently install an incomplete fresh app.

Signed light installer payloads are verified with Tauri updater; SHA-256 additionally records transfer/integrity evidence. Installer must confirm actual install and support backup/restore on failure. No public manifest points to unverified artifacts. Private keys and upload credentials stay out of source/control/logs.

## Contract: desktop updater IPC

Commands: `update_status`, `update_check` with `{manual: boolean}`, `update_install`, `update_cancel`, `update_set_channel` with `{channel: "stable" | "preview"}`. All status-returning commands return camelCase `UpdateSnapshot`.

Snapshot: `{ phase, currentVersion, channel, available, downloaded, total, error, nextCheckAt }`. Phases: `idle|checking|current|available|downloading|verifying|waiting|installing|error`. `available` is null or `{version, notes, date, downloadUrl}`; `total` and `nextCheckAt` may be null; error null or string. Emit `harmony-update-state` with same snapshot on changes. UI owns only rendering/interaction; Rust owns schedule and singleflight.

## Task 1: updater backend

Write meaningful tests first: 6h schedule persistence, failure retries5/15/60min then6h, one concurrent request, cached available survives failure, channel reset, strict CDN URL, no unverified install. Run targeted Rust tests and check. Implement single native service and main-window restricted commands. Trigger setup once. Main source runtime gate no longer requires user env to enable supported Windows updater; non-Windows gracefully unavailable. Download before shutdown, cancel while safe, wait for running tasks rather than silently terminate. Launch external light installer successfully before app exit. Check installed dependencies using payload manifest contract with installer agent. Installer exit/health evidence is separate from download success.

## Task 2: installer

Write tests first for payload inclusion/exclusion, compatibility and overwrite behavior. Build deterministic manifest from core EXE + ownedMCP + Harmony Cut scripts + licenses; reuse third-party Codex/FlaUI/Playwright, WebView2 and managed runtimes only when matching baseline. Full install base writes baseline manifest when possible. Light installer supports `/S /UPDATE /R` plus install-location discovery, no uninstall. Preserve original uninstall.exe and deep-uninstaller; update app version registry but keep uninstall identity. Back up only replaced files, atomic staging and rollback, reject unsafe paths; on manual update discover actual install, validate prerequisites, stop app safely, restart after success. Root and installer agent agree exact args and state/health files before integration.

## Task 3: frontend

Tests first for manual check results, both entrances, cancel keeps badge, event updates, errors/progress. Add singleton external store + Tauri service wrappers using above contract. About checks button + preview opt-in; app-brand badge + anchored release card; display notes as text (no executable HTML). Disable duplicate install; safe cancel only prior to handoff. Add accessible dialog/focus/Esc behavior, light/dark and collapsed sidebar. Run relevant Vitest tests then `npm test` and `npm run build:web`.

## Task 4: website

Extend existing release records/API where necessary for light URL/version/bytes/hash, backward compatible with current download sessions/metrics. Add Windows card light manual-update entry with prerequisites and explicit RC label if preview. Restrict URLs to legitimate configured origins. Targeted tests plus web build; preserve active production registration/email changes during deployment.

## Task 5: build and validation

On Windows isolated branch checkout build stable1.1.1 full, optional stable1.1.1 light for public manual stable update, and preview1.1.2-rc.1 light. Verify each embedded/core version actually matches filename, manifest and signature. Full regression (Rust/MCP/frontend/installer) then install stable into isolated test environment, detect preview via real CDN, signed download, overlay update, restart and actual version check. Compare retained file hashes and exercise MCP/skills/artifact/runtime behavior; test corrupted signature and failed install recovery. Preserve workspaces. Reviewer performs spec+quality review on final diffs.

## Task 6: publish

Upload versioned immutable objects first, full download+hash verify on CDN and Range206, then publish stable/preview JSON and portal metadata atomically where possible. Retest portal download session -> CDN and both manual light and updater routes. No RC silently offered on stable channel. Keep prior stable artifacts for recovery. Commit all task-specific source changes on new branches; final report gives paths, hashes, CDN URLs, actual tests and explicit remaining limitations.

## Progress

- [x] Version sequence confirmed.
- [x] Isolated desktop clone and 1.1.0 baseline snapshot.
- [ ] Backend.
- [ ] Overlay installer.
- [ ] UI.
- [ ] Portal.
- [ ] Review, Windows builds, install/update validation.
- [ ] CDN and portal publication verification.
