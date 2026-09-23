# RobotZ runtime replacement

## Goal

Remove RobotZ from the desktop build and replace its advertised browser and
Windows desktop capabilities with MCP servers that Codex can actually call.
Existing conversations, scheduled tasks, IM, Harvis, Office, skills, and the
right-hand preview panel must keep their current contracts.

## Current state

- `robotz-browser` creates an in-process browser manager, but neither the
  Codex app-server nor the WorkBuddy MCP Host registers `BrowserTool`.
- The browser manager passed into scheduled tasks is unused. The preview
  panel is an iframe and does not use RobotZ.
- `robotz-automation` is used by the desktop app only to load a calibration
  cache. Its UIA, screen, and keyboard tools are not registered with Codex.
- The visible builtin-tool catalog lists browser/computer-use capabilities
  even though that catalog is not the Codex `tools/list` result.

## Target architecture

1. Manage Microsoft Playwright MCP as a Codex stdio server. Ship its pinned
   Node packages with the app, use bundled Node when present, and default to
   installed Microsoft Edge on Windows. Preserve the existing headless setting.
2. Manage a pinned, self-contained FlaUI MCP as a second Codex stdio server on
   Windows. UIA3 accessibility snapshots and element references are the
   preferred desktop path; screenshots are an explicit fallback. Do not
   silently use coordinate actions for a failed semantic locator.
3. Keep WorkBuddy MCP for business tools. Preserve user-added MCP entries while
   replacing only the application's managed MCP configuration block.
4. Remove RobotZ state, startup calibration, browser availability helpers,
   unused task parameters, Rust dependencies, and vendored sources once both
   replacements pass Windows checks.

## Safety and compatibility

- MCP servers run locally over stdio, never as publicly listening services.
- Desktop mutation stays subject to Codex approval policy. Avoid duplicate
  shell/file/browser tools in the visible catalog unless they are callable.
- Do not change Tauri identifier or application data paths. Existing settings
  and sessions must survive upgrade.
- Release artifacts must be buildable offline after pinned dependencies are
  staged. Check licenses and hashes of bundled upstream binaries.

## Acceptance

- `cargo tree` and packaged binaries contain no `robotz-*` dependency.
- Codex `tools/list` exposes browser and Windows desktop MCP tools on Windows.
- Browser navigation/snapshot/click/screenshot and desktop window list/snapshot/
  click/type/screenshot pass in an interactive Windows session.
- Scheduled tasks and IM still run through Codex, and the preview panel still
  opens generated files and URLs.
- Existing frontend, Rust, MCP, and installer tests pass; full Windows package
  installs and launches on a clean user profile.

## Added regression scope (2026-09-24)

- Harvis creation results show submitted/processing status and an actual
  percentage only when Harmony returns one; otherwise show an indeterminate
  progress indicator and elapsed time, not a fabricated percentage.
- Chat Harvis tools use the same personal Harmony key, selected model, and
  Responses submission/status contract as the creation page. A bounded status
  wait must return either a finished result or a resumable response ID.
- Local images embedded in chat and interactive cards resolve through the Tauri
  asset protocol. Remote and data images remain constrained to safe schemes.
- Keep old portal job IDs readable for existing stored history; never silently
  fall back to the portal for a newly submitted Responses task.
