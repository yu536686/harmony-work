# macOS native interface

## Goal

Give the macOS build a platform-native window and navigation experience without changing the shared business flows or Windows presentation. Preserve every existing navigation destination, nested menu, session list, and command.

## Architecture

- Keep React as the owner of application navigation, conversation state, and content. Do not duplicate that state in AppKit.
- Install an AppKit `NSToolbar` on the main macOS `NSWindow`. Its sidebar and new-conversation commands dispatch DOM events into the existing React handlers. The standard window controls remain owned by macOS.
- Place `NSGlassEffectView` on macOS 26+ or `NSVisualEffectView` on earlier supported systems below the transparent main `WKWebView`. Match its width to the expanded/collapsed React sidebar. Avoid applying sidebar material to the entire window.
- Scope the Mac layout, typography, controls, motion, and accessibility fallbacks under `data-platform="macos"`. Keep content surfaces opaque; reserve translucent material for navigation and chrome.
- Sync AppKit window appearance with the persisted React light/dark choice so native glass and WebView text remain legible together.
- Use system toolbar appearance on macOS versions that support Liquid Glass and the standard AppKit appearance on older supported systems. Do not simulate refraction with decorative gradients.

## User interface

- Native unified titlebar with the system traffic lights, toolbar sidebar toggle, and new-conversation command.
- Sidebar retains Work, Collaboration, System, projects, sessions, and account areas. Improve row sizes, hierarchy, selection, disclosure, and collapsed icon state.
- Contextual page headers stay in React because they contain page-specific actions; they should read as understated Mac toolbars rather than Windows titlebars.
- Shared controls receive a Mac-only style layer: icon buttons, command buttons, fields, selects, switches, segmented controls, popovers, dialogs, and focus states. Keep standard keyboard and pointer behaviors.
- Chat and settings content use Mac spacing and type scale, avoiding glass backgrounds in the content layer.

## Acceptance

1. Windows markup, styles, packaging, and navigation behavior remain unchanged.
2. Toolbar actions and sidebar collapse operate on the same React state as the existing controls.
3. The native sidebar underlay tracks expanded and collapsed widths and window resizing.
4. Light/dark, reduced transparency, increased contrast, reduced motion, narrow window, and keyboard focus remain usable.
5. Mac frontend tests/build and Rust native tests pass; verify the native window visually on a Mac, not only in a browser.

## Current status

- Implemented the native unified toolbar, version-adaptive sidebar material, width and appearance bridge, and Mac-only typography/control styles on `feat/macos-native-shell`.
- `npm test`, `npm run test:native` (219 tests), and `npx tauri build --config src-tauri/tauri.macos.conf.json --bundles app` pass. The bundle is `target/release/bundle/macos/Harmony Work.app`.
- Visual QA on the packaged Mac app found a light native sidebar next to dark web content. The native window background, glass tint, and Mac-only sidebar surface now follow the selected appearance. Both light and dark modes were inspected in the running app.
- The native toolbar sidebar command collapsed and restored the React sidebar; the native new-conversation command opened a blank composer and focused it. The Settings page was inspected in light mode, including the Mac icon and control layout.
- Harvis music and the Skill Market were inspected for navigation and basic layout. They still contain legacy React cards, form styling, and emoji headers/icons. This phase provides a native window/navigation foundation and a Mac-specific style layer, not a complete AppKit/SwiftUI rewrite of every content page. A full page-by-page Mac control and density pass remains outstanding.
- Frontend tests (174), contract tests (28), native tests (219), and the macOS app bundle build pass after the appearance fix. The window-edge resize gesture did not resize the window during automated UI inspection, so narrow-window visual QA remains open. Reduced transparency, increased contrast, reduced motion, and keyboard-only navigation also need manual QA.
- Do not merge into the Windows mainline until the remaining accessibility/resizing checks and a Windows CI build pass.

## References

- Apple Human Interface Guidelines: Materials, Sidebars, Toolbars, Controls, Motion, Accessibility.
- Apple AppKit: `NSToolbar`, `NSVisualEffectView`, `NSGlassEffectView`.
- The supplied Figma Community macOS 27 file is a secondary visual reference. It was not accessible to the automated browser at the start of this work, so implementation decisions follow Apple documentation.
