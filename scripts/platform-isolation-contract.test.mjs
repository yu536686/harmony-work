import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const read = (relative) => readFile(path.join(root, relative), "utf8");
const json = async (relative) => JSON.parse(await read(relative));

test("Windows and macOS builds keep their native payloads separate", async () => {
  const [packageJson, base, mac] = await Promise.all([
    json("package.json"),
    json("src-tauri/tauri.conf.json"),
    json("src-tauri/tauri.macos.conf.json"),
  ]);
  assert.match(packageJson.scripts.build, /stage:flaui-mcp/);
  assert.doesNotMatch(packageJson.scripts.build, /stage:macos-runtime/);
  assert.match(packageJson.scripts["build:mac"], /stage:macos-runtime/);
  assert.doesNotMatch(packageJson.scripts["build:mac"], /stage:flaui-mcp/);
  assert.equal(base.build.beforeBuildCommand, "npm run build");
  assert.equal(mac.build.beforeBuildCommand, "npm run build:mac");
  assert.deepEqual(base.bundle.targets, ["nsis"]);
  assert.deepEqual(mac.bundle.targets, ["app", "dmg"]);
  assert.ok(base.bundle.resources.includes("resources/flaui-mcp"));
  assert.ok(base.bundle.resources.includes("resources/offline-installers"));
  assert.ok(!base.bundle.resources.some((resource) => /mac-use-mcp|resources\/node/.test(resource)));
  assert.ok(mac.bundle.resources.includes("resources/mac-use-mcp"));
  assert.ok(mac.bundle.resources.includes("resources/node"));
  for (const runtime of ["git", "python", "wecom"]) {
    assert.ok(mac.bundle.resources.includes(`resources/${runtime}`));
    assert.ok(!base.bundle.resources.includes(`resources/${runtime}`));
  }
  assert.ok(!mac.bundle.resources.some((resource) => /flaui-mcp|offline-installers/.test(resource)));
});

test("native tests exclude staged payloads without changing release bundles", async () => {
  const [nativeTest, workflow] = await Promise.all([
    read("scripts/test-native.mjs"),
    read(".github/workflows/cross-platform.yml"),
  ]);
  assert.match(nativeTest, /resources: \[\]/);
  assert.match(nativeTest, /macOSPrivateApi: true/);
  assert.match(nativeTest, /"cargo", \["test", "--locked", "--lib"\]/);
  assert.match(workflow, /windows-latest/);
  assert.match(workflow, /macos-14/);
  assert.match(workflow, /npm ci/);
  assert.match(workflow, /npm test/);
  assert.match(workflow, /npm run test:native/);
});

test("unsafe NSIS data removal is blocked in favor of workspace-safe cleanup", async () => {
  const config = await json("src-tauri/tauri.conf.json");
  const [hooks, policy, engine] = await Promise.all([
    read("src-tauri/windows/hooks.nsh"),
    read("installer/HarmonyWorkUninstaller/CleanupPolicy.cs"),
    read("installer/HarmonyWorkUninstaller/UninstallEngine.cs"),
  ]);
  assert.equal(config.bundle.windows.nsis.installerHooks, "windows/hooks.nsh");
  assert.match(hooks, /NSIS_HOOK_PREUNINSTALL/);
  assert.match(hooks, /DeleteAppDataCheckboxState = 1/);
  assert.match(hooks, /UpdateMode <> 1/);
  assert.match(hooks, /SetErrorLevel 2\s+Abort/);
  assert.doesNotMatch(hooks, /RMDir/);
  assert.match(policy, /ProtectedPaths/);
  assert.match(policy, /DataRoots/);
  assert.match(policy, /"WorkBuddy", "runtime"/);
  assert.match(engine, /CleanupPolicy\.CleanOwnedRoot/);
  assert.match(engine, /\.full-uninstall-cleaned/);
});
