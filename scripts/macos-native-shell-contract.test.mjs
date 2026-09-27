import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const source = (relative) => readFile(path.join(root, relative), "utf8");

test("macOS native shell is compiled only for macOS", async () => {
  const [manifest, build, bridge, native] = await Promise.all([
    source("src-tauri/Cargo.toml"),
    source("src-tauri/build.rs"),
    source("src-tauri/src/mac_native_shell.rs"),
    source("src-tauri/native/HWNativeShell.m"),
  ]);
  assert.match(manifest, /cfg\(target_os = "macos"\)'.build-dependencies/);
  assert.match(manifest, /\[profile\.release\.build-override\]\s*strip = "none"/);
  assert.match(build, /cfg\(target_os = "macos"\)/);
  assert.match(build, /HWNativeShell\.m/);
  assert.match(bridge, /cfg\(target_os = "macos"\)/);
  assert.match(native, /NSToolbar/);
  assert.match(native, /NSGlassEffectView/);
  assert.match(native, /NSVisualEffectView/);
  assert.match(native, /NSClassFromString\(@"NSGlassEffectView"\)/);
  assert.doesNotMatch(native, /@available/);
  assert.match(native, /window\.appearance = appearance/);
});

test("macOS sidebar and toolbar actions reuse React navigation", async () => {
  const [app, css, config] = await Promise.all([
    source("src/App.tsx"),
    source("src/macos.css"),
    source("src-tauri/tauri.macos.conf.json"),
  ]);
  assert.match(app, /harmony:native-toggle-sidebar/);
  assert.match(app, /harmony:native-new-chat/);
  assert.match(app, /set_macos_shell_state/);
  assert.match(app, /dark: colorMode === "dark"/);
  assert.match(css, /data-platform="macos"/);
  assert.match(css, /prefers-reduced-transparency/);
  assert.match(css, /prefers-reduced-motion/);
  assert.doesNotMatch(config, /"windowEffects"/);
});
