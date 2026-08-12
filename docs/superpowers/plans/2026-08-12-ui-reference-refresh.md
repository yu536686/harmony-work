# UI Reference Refresh Implementation Plan

**Goal:** Apply the approved contemporary WorkBuddy visual direction without changing service, state, permission, login, or module routing behavior.

## Scope

1. Add `design.md` as the project-level visual source of truth.
2. Replace the application-shell token set and layout styling.
3. Add an application sidebar collapsed state while preserving every tab and navigation target.
4. Rework the chat empty state, session rail, composer and visual motion treatment.
5. Bump to `0.8.6-alpha.0`, test, build the NSIS bundle, and commit the branch.

## Guardrails

- Keep the existing React routes, lazy components, Redux store, Tauri service APIs and login gates intact.
- Keep inactive tabs mounted after first use, as implemented today.
- Avoid backend, database, gateway and skill-runtime edits.
- Preserve light/dark mode and existing theme preference storage.

## Verification

1. Run focused token tests after implementation.
2. Run `npm test` and `npm run build:web`.
3. Run `npx tauri build --bundles nsis` under the Windows VS build environment.
4. Inspect the generated installer path/version and git diff before commit.
