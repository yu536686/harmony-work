import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const json = async (relative) => JSON.parse(await readFile(path.join(root, relative), "utf8"));

test("macOS build overrides Windows-only staging and bundle payloads", async () => {
  const [packageJson, windowsConfig, macConfig] = await Promise.all([
    json("package.json"),
    json("src-tauri/tauri.conf.json"),
    json("src-tauri/tauri.macos.conf.json"),
  ]);
  assert.match(packageJson.scripts.build, /stage:flaui-mcp/);
  assert.doesNotMatch(packageJson.scripts["build:mac"], /stage:flaui-mcp/);
  assert.match(packageJson.scripts["build:mac"], /stage:macos-runtime/);
  assert.equal(macConfig.build.beforeBuildCommand, "npm run build:mac");
  assert.deepEqual(macConfig.bundle.targets, ["app", "dmg"]);
  assert.ok(macConfig.bundle.resources.includes("resources/node"));
  assert.ok(macConfig.bundle.resources.includes("resources/mac-use-mcp"));
  assert.ok(!macConfig.bundle.resources.some((resource) => /flaui|offline-installers/.test(resource)));
  assert.ok(windowsConfig.bundle.resources.includes("resources/flaui-mcp"));
  assert.equal(macConfig.app.windows.find((window) => window.label === "main").minWidth, 980);
  const main = macConfig.app.windows.find((window) => window.label === "main");
  assert.equal(main.transparent, true);
  assert.ok(main.windowEffects.effects.includes("sidebar"));
  assert.equal(macConfig.bundle.macOS.minimumSystemVersion, "13.0");
  assert.equal(macConfig.app.macOSPrivateApi, true);
});

test("macOS desktop automation is optional on Windows installs", async () => {
  const [packageJson, lockfile] = await Promise.all([json("package.json"), json("package-lock.json")]);
  assert.equal(packageJson.optionalDependencies["mac-use-mcp"], "^1.1.1");
  assert.equal(lockfile.packages["node_modules/mac-use-mcp"].optional, true);
  assert.deepEqual(lockfile.packages["node_modules/mac-use-mcp"].os, ["darwin"]);
});
