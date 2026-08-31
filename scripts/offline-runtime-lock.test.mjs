import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const lock = JSON.parse(
  await readFile(new URL("../installer/runtime-lock.json", import.meta.url), "utf8"),
);

test("offline runtime lock pins every required Windows payload to HTTPS and SHA-256", () => {
  assert.equal(lock.schemaVersion, 1);
  assert.equal(lock.platform, "win-x64");

  const required = ["node", "python", "portableGit", "wecomCli", "wecomCliWindows"];
  for (const key of required) {
    const item = lock.components[key];
    assert.ok(item, `missing ${key}`);
    assert.match(item.fileName, /\S/);
    assert.match(item.url, /^https:\/\//);
    assert.match(item.sha256, /^[a-f0-9]{64}$/i);
  }
});

test("offline runtime lock includes a fully pinned Harmony Cut wheelhouse", () => {
  assert.ok(Array.isArray(lock.pythonWheels));
  assert.ok(lock.pythonWheels.length >= 6);
  for (const wheel of lock.pythonWheels) {
    assert.match(wheel.name, /^[a-z0-9-]+$/i);
    assert.match(wheel.version, /^\d+\./);
    assert.match(wheel.fileName, /\.whl$/);
    assert.match(wheel.sha256, /^[a-f0-9]{64}$/i);
  }
});

test("Windows full installer resolves the Cargo target directory and retains PATH storage", async () => {
  const [processSource, installerScript, workflow] = await Promise.all([
    readFile(new URL("../src-tauri/src/process.rs", import.meta.url), "utf8"),
    readFile(new URL("./build-full-installer.ps1", import.meta.url), "utf8"),
    readFile(new URL("../.github/workflows/build-windows.yml", import.meta.url), "utf8"),
  ]);

  assert.match(
    processSource,
    /if let Some\(current_path\) = std::env::var_os\("PATH"\)/,
    "the Windows PATH value must live while split_paths borrows it",
  );
  assert.match(
    installerScript,
    /Join-Path \$ProjectRoot "target"/,
    "the bootstrapper must search Cargo's repository-level target directory",
  );
  assert.match(
    workflow,
    /^\s+target\/x86_64-pc-windows-msvc\/release\/bundle\/nsis\/\*\.exe$/m,
    "the uploaded NSIS artifact must use Cargo's repository-level target directory",
  );
  assert.match(
    installerScript,
    /\$global:LASTEXITCODE\s*=\s*0\s*\n\s*& \(Join-Path \$PSScriptRoot "fetch-offline-runtime\.ps1"\)/,
    "the full installer must not inherit a stale native exit code before fetching cached payloads",
  );
});
