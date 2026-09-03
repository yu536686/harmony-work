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
  const [processSource, installerScript] = await Promise.all([
    readFile(new URL("../src-tauri/src/process.rs", import.meta.url), "utf8"),
    readFile(new URL("./build-full-installer.ps1", import.meta.url), "utf8"),
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
    installerScript,
    /\$global:LASTEXITCODE\s*=\s*0\s*\n\s*& \(Join-Path \$PSScriptRoot "fetch-offline-runtime\.ps1"\)/,
    "the full installer must not inherit a stale native exit code before fetching cached payloads",
  );
  assert.match(
    installerScript,
    /\$global:LASTEXITCODE\s*=\s*0\s*\n\s*& \(Join-Path \$PSScriptRoot "stage-full-installer-payload\.ps1"\)/,
    "the full installer must not inherit a stale native exit code before staging the payload",
  );
});

test("WPF bootstrapper declares the installer main window as its startup window", async () => {
  const appXaml = await readFile(
    new URL("../installer/WorkBuddyBootstrapper/App.xaml", import.meta.url),
    "utf8",
  );

  assert.match(
    appXaml,
    /StartupUri="MainWindow\.xaml"/,
    "the bootstrapper must create MainWindow instead of remaining as a background process",
  );
});

test("WPF bootstrapper defers the large offline payload until after its window is visible", async () => {
  const [project, engine] = await Promise.all([
    readFile(
      new URL("../installer/WorkBuddyBootstrapper/WorkBuddyBootstrapper.csproj", import.meta.url),
      "utf8",
    ),
    readFile(
      new URL("../installer/WorkBuddyBootstrapper/InstallerEngine.cs", import.meta.url),
      "utf8",
    ),
  ]);

  assert.match(
    project,
    /<EmbeddedResource Include="\$\(PayloadArchive\)" LogicalName="WorkBuddyBootstrapper\.PayloadArchive"/,
    "the payload must be embedded as one archive instead of extracted before WPF starts",
  );
  assert.doesNotMatch(
    project,
    /IncludeAllContentForSelfExtract>true/,
    "self-extracting all content blocks the installer window behind the large payload extraction",
  );
  assert.match(
    engine,
    /GetManifestResourceStream\("WorkBuddyBootstrapper\.PayloadArchive"\)/,
    "the installer must materialize the embedded payload only after the user starts installation",
  );
});

test("WPF bootstrapper shows readiness before installation and exits after launch", async () => {
  const [window, code] = await Promise.all([
    readFile(new URL("../installer/WorkBuddyBootstrapper/MainWindow.xaml", import.meta.url), "utf8"),
    readFile(new URL("../installer/WorkBuddyBootstrapper/MainWindow.xaml.cs", import.meta.url), "utf8"),
  ]);

  assert.match(window, /Content="正在加载…" IsEnabled="False"/);
  assert.match(code, /Loaded \+= Window_Loaded/);
  assert.match(code, /WarmUpAsync\(CancellationToken\.None\)/);
  assert.match(code, /Application\.Current\.Shutdown\(\)/);
  assert.match(code, /_installCancellation\?\.Cancel\(\)/);
  assert.match(code, /_closeAfterCancellation/);
});

test("full installer reuses managed runtimes and reports generic user-facing progress", async () => {
  const engine = await readFile(
    new URL("../installer/WorkBuddyBootstrapper/InstallerEngine.cs", import.meta.url),
    "utf8",
  );

  assert.match(engine, /if \(!HasNodeRuntime\(\)\) InstallNode/);
  assert.match(engine, /if \(!HasPythonRuntime\(\)\) await InstallPythonAsync/);
  assert.match(engine, /if \(!HasGitRuntime\(\)\) await InstallPortableGitAsync/);
  assert.match(engine, /if \(!HasWeComCliRuntime\(\)\) InstallWeComCli/);
  assert.match(engine, /"正在配置必要组件", ""/);
  assert.match(engine, /CopyPayloadArchiveAsync/);
  assert.match(engine, /ExtractPayloadArchiveAsync/);
});
