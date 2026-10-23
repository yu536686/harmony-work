import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const scriptUrl = new URL("./stage-macos-python.mjs", import.meta.url);
const runtime = await import(scriptUrl).catch(() => null);
const content = Buffer.from("test wheel payload");
const wheel = {
  name: "starlette", version: "1.6.0", fileName: "starlette-1.6.0-py3-none-any.whl",
  url: "https://files.pythonhosted.org/packages/starlette-1.6.0-py3-none-any.whl",
  sha256: createHash("sha256").update(content).digest("hex"),
};
const lock = () => ({
  schemaVersion: 1, platform: "darwin-arm64", pythonVersion: "3.12.12",
  minimumMacosVersion: "13.5", pythonWheels: [{ ...wheel }],
});

async function temporary(t) {
  const directory = await mkdtemp(join(tmpdir(), "harmony-macos-python-test-"));
  t.after(() => rm(directory, { recursive: true, force: true }));
  return directory;
}

test("stager exports its testable API without running staging on import", () => {
  assert.ok(runtime, "macOS Python staging implementation is missing");
  for (const name of ["parseArgs", "validateLock", "ensureWheel", "stageMacosPython"])
    assert.equal(typeof runtime[name], "function", name);
});

test("lock refresh is explicit and cannot be combined with offline mode", () => {
  assert.ok(runtime);
  assert.deepEqual(runtime.parseArgs([]), { refreshLock: false, offline: false, help: false });
  assert.equal(runtime.parseArgs(["--refresh-lock"]).refreshLock, true);
  assert.equal(runtime.parseArgs(["--offline"]).offline, true);
  assert.throws(() => runtime.parseArgs(["--refresh-lock", "--offline"]), /offline/i);
  assert.throws(() => runtime.parseArgs(["--typo"]), /unknown/i);
});

test("lock rejects foreign platforms, paths, sources, duplicates, and missing hashes", () => {
  assert.ok(runtime);
  runtime.validateLock(lock());
  for (const patch of [
    { fileName: "../escape.whl" }, { fileName: "windows-win_amd64.whl" },
    { fileName: "package-macosx_14_0_arm64.whl" },
    { url: "http://files.pythonhosted.org/a.whl" }, { sha256: "" },
    { url: "https://files.pythonhosted.org/not-the-locked-file.whl" },
  ]) {
    const invalid = lock();
    Object.assign(invalid.pythonWheels[0], patch);
    assert.throws(() => runtime.validateLock(invalid));
  }
  assert.throws(() => runtime.validateLock({ ...lock(), platform: "win-x64" }));
  assert.throws(() => runtime.validateLock({ ...lock(), pythonWheels: [wheel, wheel] }));
});

test("verified cached wheels need no downloader, even offline", async (t) => {
  assert.ok(runtime);
  const cache = await temporary(t);
  await writeFile(join(cache, wheel.fileName), content);
  const path = await runtime.ensureWheel(wheel, cache, {
    offline: true, download: () => assert.fail("offline cache must not access the network"),
  });
  assert.equal(path, join(cache, wheel.fileName));
});

test("missing offline wheel fails without network access", async (t) => {
  assert.ok(runtime);
  const cache = await temporary(t);
  await assert.rejects(runtime.ensureWheel(wheel, cache, {
    offline: true, download: () => assert.fail("offline must not download"),
  }), /offline.*missing/i);
});

test("corrupted cache fails closed instead of replacing a pinned artifact", async (t) => {
  assert.ok(runtime);
  const cache = await temporary(t);
  await writeFile(join(cache, wheel.fileName), "tampered");
  await assert.rejects(runtime.ensureWheel(wheel, cache, {
    download: () => assert.fail("corruption must not trigger a silent repair"),
  }), /sha-?256.*mismatch/i);
  assert.equal(await readFile(join(cache, wheel.fileName), "utf8"), "tampered");
});

test("cold downloads are hashed before they enter the reusable cache", async (t) => {
  assert.ok(runtime);
  const cache = await temporary(t);
  let called = false;
  await runtime.ensureWheel(wheel, cache, {
    download: async (item, destination) => {
      called = true;
      assert.deepEqual(item, wheel);
      assert.notEqual(destination, cache);
      await writeFile(join(destination, item.fileName), content);
    },
  });
  assert.equal(called, true);
  assert.deepEqual(await readFile(join(cache, wheel.fileName)), content);
  assert.deepEqual(await readdir(cache), [wheel.fileName]);
});

test("wrong downloaded bytes never become a cached wheel", async (t) => {
  assert.ok(runtime);
  const cache = await temporary(t);
  await assert.rejects(runtime.ensureWheel(wheel, cache, {
    download: (item, destination) => writeFile(join(destination, item.fileName), "bad"),
  }), /sha-?256.*mismatch/i);
  assert.deepEqual(await readdir(cache), []);
});

test("committed Mac lock preserves the Windows package/version selection", async () => {
  const mac = JSON.parse(await readFile(new URL("../installer/runtime-lock.macos.json", import.meta.url)));
  const windows = JSON.parse(await readFile(new URL("../installer/runtime-lock.json", import.meta.url)));
  assert.ok(runtime);
  runtime.validateLock(mac);
  assert.deepEqual(mac.pythonWheels.map(({ name, version }) => ({ name, version })),
    windows.pythonWheels.map(({ name, version }) => ({ name, version })));
  for (const wheel of mac.pythonWheels) {
    assert.match(wheel.fileName, /(?:none-any|macosx_(?:11|12|13)_0_arm64)\.whl$/);
  }
});

const integration = process.env.HARMONY_MACOS_PYTHON_INTEGRATION === "1";

test("integration: cold cache fetches the exact locked HTTPS wheel", { skip: !integration }, async (t) => {
  const mac = JSON.parse(await readFile(new URL("../installer/runtime-lock.macos.json", import.meta.url)));
  const item = mac.pythonWheels.find(({ name }) => name === "starlette");
  const directory = await temporary(t);
  const path = await runtime.ensureWheel(item, directory);
  assert.equal(createHash("sha256").update(await readFile(path)).digest("hex"), item.sha256);
});

test("integration: warm staging works with network denied and leaves the lock unchanged", { skip: !integration }, async () => {
  const lockUrl = new URL("../installer/runtime-lock.macos.json", import.meta.url);
  const before = await readFile(lockUrl);
  for (const flags of [[], ["--offline"]]) {
    const result = spawnSync("/usr/bin/sandbox-exec", [
      "-p", "(version 1)(allow default)(deny network*)", process.execPath,
      fileURLToPath(scriptUrl), ...flags,
    ], { encoding: "utf8", timeout: 120_000, maxBuffer: 8 * 1024 * 1024,
      env: { ...process.env, PYTHONHOME: "/nonexistent", PYTHONPATH: "/nonexistent", IMAGEIO_FFMPEG_EXE: "/nonexistent" } });
    assert.equal(result.status, 0, `${result.error ?? ""}\n${result.stdout}\n${result.stderr}`);
    assert.match(result.stdout, /Staged 14 Harmony Cut wheels/);
    assert.match(result.stdout, /ffmpeg version 7\.1/);
    assert.deepEqual(await readFile(lockUrl), before);
  }
});

test("integration: installed metadata and license files remain inside app Python", { skip: !integration }, () => {
  const result = spawnSync(fileURLToPath(new URL("../src-tauri/resources/python/bin/python3", import.meta.url)), [
    "-I", "-B", "-c", String.raw`
import importlib.metadata, json, sys
from pathlib import Path
root = Path(sys.argv[1])
lock = json.loads((root / 'installer/runtime-lock.macos.json').read_text())
site = root / 'src-tauri/resources/python/lib/python3.12/site-packages'
for wheel in lock['pythonWheels']:
    distribution = importlib.metadata.distribution(wheel['name'])
    assert distribution.version == wheel['version']
    assert Path(distribution.locate_file('')).resolve() == site.resolve()
    licenses = [file for file in distribution.files if 'license' in str(file).lower() or 'copying' in str(file).lower()]
    assert licenses, wheel['name']
    for license in licenses:
        assert Path(distribution.locate_file(license)).is_file(), license
sys.path.insert(0, str(root / 'src-tauri/resources/harmony-cut'))
import server
assert len(server.app.routes) >= 10
print('All 14 package versions, licenses, and Harmony Cut ASGI imports verified')
`, fileURLToPath(new URL("..", import.meta.url)),
  ], { encoding: "utf8", timeout: 30_000 });
  assert.equal(result.status, 0, `${result.error ?? ""}\n${result.stdout}\n${result.stderr}`);
});
