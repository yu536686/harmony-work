# Cross-platform development foundation

## Restore point

- Baseline commit: `523eaafa3afbcbca1a064c279292aa386a5022d6`.
- Git tag: `restore/pre-cross-platform-foundation-20260927`.
- A verified full Git bundle is stored outside the repository; its location is recorded in the task handoff, not in the public repository.
- Windows and macOS use separate checkouts and separate dependency directories.
- Restore source in a new checkout with `git clone <bundle-file> <destination>` and `git switch --detach 523eaafa3afbcbca1a064c279292aa386a5022d6`. Do not reset an active checkout that may contain later work.

## Boundaries

- One GitHub repository and one integration branch (`main`). Platform feature branches are short-lived; platform-specific checkouts never share `node_modules`, Rust `target`, or staged resources.
- Shared business behavior stays in shared React/Rust modules. OS-specific shell, desktop automation, browser defaults, local paths, permissions, and package payloads are selected through a tested platform profile or explicit target-specific implementation.
- The base Tauri config carries Windows packaging; `tauri.macos.conf.json` overrides the macOS window and bundle. Every platform bundle contract must assert both inclusion of its own payload and exclusion of the other platform's payload.
- UI copy should describe the current platform or stay platform-neutral. A Mac-only permission prompt must not appear on Windows.

## Merge gates

1. Run `npm ci`, `npm test`, and `npm run build:web` in independent Windows and macOS checkouts.
2. Run `npm run test:native` on both operating systems. It invokes `cargo test --locked --lib` with test-only Tauri config that excludes unstaged release payloads and macOS private window effects. Pure platform-profile tests assert Windows and macOS values on either host.
3. Verify platform build contracts before merging. Release packaging remains native: Windows NSIS on Windows, macOS app/DMG on macOS.
4. Smoke-test shared user paths on both builds: login/config, chat and attachments, artifacts/preview, skills, Harvis, browser and desktop automation. A passing compile is not a substitute for these release checks.

## Implementation checklist

- [x] Create and verify a recoverable baseline before edits.
- [x] Separate Windows and macOS working copies and dependency trees.
- [x] Add host-independent platform defaults and regression tests.
- [x] Keep Windows artifact URI comparison case-insensitive and resolve existing path aliases while supporting macOS paths.
- [x] Add two-OS CI and packaging contracts.
- [x] Validate macOS locally: frontend and contract tests, production frontend build, 219 native unit tests.
- [x] Validate Windows in CI on commit `5d0a62e`: frontend and contract tests, production frontend build, native unit tests.
- [ ] Complete macOS hosted CI on the final code. An earlier macOS run passed; the run for `5d0a62e` passed frontend/build but stalled during native tests and was manually cancelled.
- [ ] Smoke-test native release bundles on both platforms before merging to `main` or shipping an installer.
