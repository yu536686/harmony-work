# Harmony Work macOS release ledger

Branch: `feat/macos-liquid-glass` in the existing `harmony-work` repository.
Windows release behavior must remain unchanged. Agent identity remains Xiaoheng.

## Acceptance gates

- [x] Native Apple Silicon app and unsigned DMG build from the same sources.
- [ ] Intel build verified separately.
- [ ] Existing chat, projects, IM, skills, market, Harvis, media library, account sync, settings, artifacts, and annotation flows work on macOS.
- [ ] Codex shell and embedded terminal use the user's macOS shell with working PTY, approvals, and workspace boundaries.
- [ ] Browser and desktop automation have macOS implementations with permission onboarding and end-to-end tests. Bundled MCP protocol is tested; OS permission and real-tool flows are not.
- [ ] Node, Python, browser, and Harmony Cut dependencies are packaged or clearly installed by the app; no Windows payloads in the Mac bundle.
- [ ] Mac-specific UI follows Apple HIG: native window controls, sidebar and toolbar material hierarchy, system typography, light/dark appearance, contrast and reduced-transparency support.
- [ ] Drag/drop, file previews, links, tray, notifications, and window lifecycle verified in a signed/notarized production install.

## Current findings

- Existing `build-mac.sh` references an obsolete local checkout. Default build runs Windows-only FlaUI staging and includes Windows installers.
- Codex already ships Darwin sidecars and handles shell commands; the embedded PTY still falls back to bash when launched from Finder.
- Browser discovery returns none on non-Windows, so bundled Playwright MCP is never registered on Mac.
- Skill compatibility currently assumes the current OS is Windows.
- macOS desktop automation has no managed MCP backend; Windows FlaUI must not be silently reported as supported.
- Apple Silicon app and unsigned DMG built on this Mac with Command Line Tools. App launch and navigation were visually checked. Code signing/notarization and installed-app regression are still unverified.
- Bundled Node is pinned to v24.18.0; update its tracked license resource in lockstep with the binary version.
- Bundled mac-use-mcp 1.1.1 responds to MCP initialize and lists 18 tools. Accessibility and Screen Recording must be granted by the user in macOS System Settings; first-use UX and real UI actions still need testing.
- `npm test` passed (28 Vitest files / 174 tests and 23 Node contract tests) in the Mac staging checkout. `cargo test --release` passed 215/215 Rust tests after fixing artifact URI normalization for `/var` and `/private/var` aliases.
- The macOS 13 minimum version is present in the built app's Info.plist. The final app/DMG includes the artifact path and system-appearance changes. The unsigned preview DMG is at `release/macos-preview/Harmony Work_1.0.6_aarch64.dmg` (SHA-256 `099b0664225b1880f6c6f0f1f9779c7236129b10488f6a5030f81c11fae0dc94`).
- Harmony Cut currently hardcodes a Windows root, invokes `python`, and expects `USERPROFILE` for the JianYing draft skill. Do not expose it as Mac-ready until the Python runtime, draft integration, and video flows are ported and tested.
- Browser automation requires Chrome or Edge installed in `/Applications` or `~/Applications`. Browser MCP registration and real navigation still need a packaged-app test.

## Implementation sequence

1. Platform-safe build profile, resource staging, and Mac boot smoke test.
2. Shell/browser/runtime parity and Mac desktop automation with permission UX.
3. Liquid Glass navigation/window layer and responsive content QA.
4. Feature-by-feature regression matrix, unsigned DMG, then signing/notarization and release.

Do not merge or publish a macOS release until every acceptance gate passes. Record command outputs and blockers here as work continues.

## Feature regression matrix

| Area | Current macOS evidence | Next verification |
| --- | --- | --- |
| Window and navigation | App launches and renders sidebar/chat | Check light/dark, 980px minimum, full-screen, reduced transparency, close/reopen |
| Chat and Codex shell | Darwin Codex sidecar staged; embedded PTY defaults to zsh | Run a prompt with shell approval and workspace boundary on a disposable project |
| Files and results | Shared Tauri file/preview code compiles | Drag from Finder; preview PDF/image; annotate; reopen task |
| Browser automation | Chrome/Edge detection added; MCP bundled | Launch browser from installed app; navigate and capture screenshot |
| Desktop automation | mac-use-mcp bundled with universal helper | Grant OS permissions; `check_permissions`, list windows, click, screenshot |
| Skills and market | OS compatibility check corrected | Install, run, update, uninstall a skill |
| Harvis and assets | Shared UI builds | Submit music/image tasks; poll, preview, archive; browse NAS assets |
| Account, IM, settings | Shared UI builds | Login, sync, WeCom/WeChat connection, restart-sensitive settings |
| Harmony Cut | Windows-only dependency chain found | Port Python runtime and draft output, then test preview/export |
| Distribution | Unsigned Apple Silicon DMG produced | Intel build, codesign, notarize, install on a clean Mac |
