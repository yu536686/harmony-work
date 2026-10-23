import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { chmod, lstat, mkdir, mkdtemp, readFile, readdir, realpath, rename, rm, writeFile } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const pythonRoot = join(root, "src-tauri/resources/python");
const python = join(pythonRoot, "bin/python3");
const sitePackages = join(pythonRoot, "lib/python3.12/site-packages");
const cache = join(root, "installer/payload-cache-macos/python-wheels");
const lockPath = join(root, "installer/runtime-lock.macos.json");
const targetArgs = ["--platform", "macosx_13_0_arm64", "--python-version", "3.12",
  "--implementation", "cp", "--abi", "cp312", "--abi", "abi3", "--abi", "none"];
const normalize = (name) => name.toLowerCase().replace(/[-_.]+/g, "-");
const environment = () => {
  const env = { ...process.env, PIP_CONFIG_FILE: "/dev/null", IMAGEIO_NO_INTERNET: "1" };
  for (const key of ["PYTHONHOME", "PYTHONPATH", "IMAGEIO_FFMPEG_EXE"]) delete env[key];
  return env;
};

export function parseArgs(args) {
  const options = { refreshLock: false, offline: false, help: false };
  for (const arg of args) {
    if (arg === "--refresh-lock") options.refreshLock = true;
    else if (arg === "--offline") options.offline = true;
    else if (arg === "--help") options.help = true;
    else throw new Error(`Unknown option: ${arg}`);
  }
  if (options.refreshLock && options.offline) throw new Error("--refresh-lock cannot run offline");
  return options;
}

export function validateLock(lock) {
  if (lock.schemaVersion !== 1 || lock.platform !== "darwin-arm64" ||
      lock.pythonVersion !== "3.12.12" || lock.minimumMacosVersion !== "13.5" ||
      !Array.isArray(lock.pythonWheels) || !lock.pythonWheels.length) {
    throw new Error("Invalid macOS Python runtime lock");
  }
  const names = new Set();
  for (const wheel of lock.pythonWheels) {
    const source = new URL(wheel.url);
    if (!/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(wheel.name) || names.has(wheel.name) ||
        !/^\d+(?:\.\d+)+$/.test(wheel.version) ||
        !/^[a-zA-Z0-9_.+-]+\.whl$/.test(wheel.fileName) ||
        !/(?:-none-any|-macosx_(?:11|12|13)_0_(?:arm64|universal2))\.whl$/.test(wheel.fileName) ||
        source.protocol !== "https:" || source.username || source.password || source.search || source.hash ||
        basename(source.pathname) !== wheel.fileName || !/^[a-f0-9]{64}$/.test(wheel.sha256)) {
      throw new Error(`Invalid locked macOS wheel: ${wheel.name}`);
    }
    names.add(wheel.name);
  }
}

async function sha256(path) {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(path)) hash.update(chunk);
  return hash.digest("hex");
}

async function verifyHash(path, expected) {
  if (await sha256(path) !== expected) {
    throw new Error(`SHA-256 mismatch: ${path}. Remove the corrupt cache file explicitly before retrying.`);
  }
}

function runPython(args, { input, capture = false } = {}) {
  const result = spawnSync(python, ["-I", "-B", ...args], {
    cwd: root, env: environment(), encoding: "utf8", input,
    stdio: capture || input !== undefined ? "pipe" : "inherit",
    maxBuffer: 8 * 1024 * 1024,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`Bundled Python failed (exit ${result.status ?? result.signal})${result.stderr ? `:\n${result.stderr}` : ""}`);
  }
  return result.stdout;
}

function pip(args) {
  return runPython(["-m", "pip", "--isolated", "--disable-pip-version-check", ...args]);
}

function downloadWheel(wheel, destination) {
  pip(["download", "--only-binary=:all:", "--no-deps", "--no-cache-dir", "--no-index",
    ...targetArgs, "--dest", destination, `${wheel.url}#sha256=${wheel.sha256}`]);
}

export async function ensureWheel(wheel, directory, { offline = false, download = downloadWheel } = {}) {
  await mkdir(directory, { recursive: true });
  const destination = join(directory, wheel.fileName);
  try {
    const stat = await lstat(destination);
    if (!stat.isFile()) throw new Error(`Wheel cache entry is not a regular file: ${destination}`);
    await verifyHash(destination, wheel.sha256);
    return destination;
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
  }
  if (offline) throw new Error(`Offline wheel missing: ${destination}`);
  const temporary = await mkdtemp(join(directory, ".download-"));
  try {
    await download(wheel, temporary);
    await verifyHash(join(temporary, wheel.fileName), wheel.sha256);
    await rename(join(temporary, wheel.fileName), destination);
    return destination;
  } finally {
    await rm(temporary, { recursive: true, force: true });
  }
}

async function refreshLock() {
  const windows = JSON.parse(await readFile(join(root, "installer/runtime-lock.json"), "utf8"));
  await mkdir(cache, { recursive: true });
  const temporary = await mkdtemp(join(cache, ".resolve-"));
  try {
    // Resolve the exact Windows versions, but constrain native wheels to macOS <= 13.5.
    pip(["download", "--only-binary=:all:", "--no-deps", "--no-cache-dir",
      "--index-url", "https://pypi.org/simple", ...targetArgs, "--dest", temporary,
      ...windows.pythonWheels.map(({ name, version }) => `${name}==${version}`)]);
    const filenames = await readdir(temporary);
    const pythonWheels = [];
    for (const { name, version } of windows.pythonWheels) {
      const response = await fetch(`https://pypi.org/pypi/${name}/${version}/json`);
      if (!response.ok) throw new Error(`PyPI metadata unavailable: ${name}==${version} (${response.status})`);
      const metadata = await response.json();
      const matches = metadata.urls.filter((item) => filenames.includes(item.filename) && item.packagetype === "bdist_wheel");
      if (matches.length !== 1) throw new Error(`Expected one macOS wheel for ${name}==${version}`);
      const item = matches[0];
      if (item.yanked) throw new Error(`Refusing yanked wheel: ${item.filename}`);
      const wheel = { name, version, fileName: item.filename, url: item.url, sha256: item.digests.sha256 };
      await verifyHash(join(temporary, wheel.fileName), wheel.sha256);
      pythonWheels.push(wheel);
    }
    const lock = { schemaVersion: 1, platform: "darwin-arm64", pythonVersion: "3.12.12",
      minimumMacosVersion: "13.5", versionSource: "installer/runtime-lock.json#pythonWheels", pythonWheels };
    validateLock(lock);
    for (const wheel of pythonWheels) {
      await ensureWheel(wheel, cache, {
        download: (_, destination) => rename(join(temporary, wheel.fileName), join(destination, wheel.fileName)),
      });
    }
    return lock;
  } finally {
    await rm(temporary, { recursive: true, force: true });
  }
}

const verifyWheels = String.raw`
import json, sys, zipfile
from email.parser import BytesParser
from pathlib import Path
from pip._vendor.packaging.requirements import Requirement
from pip._vendor.packaging.specifiers import SpecifierSet
from pip._vendor.packaging.tags import cpython_tags, compatible_tags, mac_platforms, parse_tag
from pip._vendor.packaging.utils import canonicalize_name

lock = json.load(sys.stdin)
versions = {canonicalize_name(w['name']): w['version'] for w in lock['pythonWheels']}
platforms = list(mac_platforms((13, 0), 'arm64'))
supported = set(cpython_tags((3, 12), abis=['cp312'], platforms=platforms))
supported.update(compatible_tags((3, 12), interpreter='cp312', platforms=platforms))
for wheel in lock['pythonWheels']:
    with zipfile.ZipFile(Path(sys.argv[1]) / wheel['fileName']) as archive:
        files = archive.namelist()
        metadata_files = [p for p in files if p.endswith('.dist-info/METADATA')]
        assert len(metadata_files) == 1, wheel['fileName']
        dist_info = metadata_files[0].rsplit('/', 1)[0]
        assert dist_info + '/RECORD' in files, wheel['name']
        assert any(('license' in p.lower() or 'copying' in p.lower()) and not p.endswith('/') for p in files), 'Missing license: ' + wheel['name']
        metadata = BytesParser().parsebytes(archive.read(metadata_files[0]))
        assert canonicalize_name(metadata['Name']) == wheel['name'], wheel['name']
        assert metadata['Version'] == wheel['version'], wheel['name']
        assert SpecifierSet(metadata.get('Requires-Python', '')).contains('3.12.12'), wheel['name']
        wheel_metadata = BytesParser().parsebytes(archive.read(dist_info + '/WHEEL'))
        tags = set().union(*(parse_tag(t) for t in wheel_metadata.get_all('Tag', [])))
        assert tags & supported, 'Incompatible wheel: ' + wheel['fileName']
        for value in metadata.get_all('Requires-Dist', []):
            dependency = Requirement(value)
            if dependency.marker and not dependency.marker.evaluate({'extra': ''}):
                continue
            name = canonicalize_name(dependency.name)
            assert name in versions, 'Unpinned dependency: ' + value
            assert dependency.specifier.contains(versions[name]), 'Incompatible dependency: ' + value
print('Verified wheel metadata, dependency closure, macOS tags, and licenses')
`;

const smokeCode = String.raw`
import importlib, importlib.metadata, json, os, subprocess, sys
from pathlib import Path

target = Path(sys.argv[1]).resolve()
sys.path.insert(0, str(target))
versions = json.load(sys.stdin)
for name, version in versions.items():
    distribution = importlib.metadata.distribution(name)
    assert distribution.version == version, (name, distribution.version, version)
    assert Path(distribution.locate_file('')).resolve() == target, name
for name in ['starlette', 'uvicorn', 'pydantic', 'cv2', 'numpy', 'imageio_ffmpeg']:
    module = importlib.import_module(name)
    assert Path(module.__file__).resolve().is_relative_to(target), module.__file__
    print(name + ': ' + getattr(module, '__version__', 'import OK'))
import numpy as np, cv2, imageio_ffmpeg
from pydantic import BaseModel
from starlette.applications import Starlette
class Payload(BaseModel):
    frames: int
assert Payload(frames='2').frames == 2
assert cv2.cvtColor(np.zeros((2, 2, 3), dtype=np.uint8), cv2.COLOR_BGR2GRAY).shape == (2, 2)
assert Starlette().debug is False
binary = Path(imageio_ffmpeg.get_ffmpeg_exe()).resolve()
assert binary.is_relative_to(target / 'imageio_ffmpeg' / 'binaries'), binary
assert os.access(binary, os.X_OK), binary
result = subprocess.run([str(binary), '--version'], capture_output=True, text=True)
# ffmpeg conventionally uses -version; some builds reject the GNU spelling.
if result.returncode:
    result = subprocess.run([str(binary), '-version'], capture_output=True, text=True, check=True)
assert 'ffmpeg version' in result.stdout, result.stderr
print('Bundled ffmpeg: ' + str(binary))
print(result.stdout.splitlines()[0])
`;

function smoke(target, lock) {
  const output = runPython(["-c", smokeCode, target], {
    input: JSON.stringify(Object.fromEntries(lock.pythonWheels.map(({ name, version }) => [name, version]))),
  });
  console.log(output.trim());
}

async function install(lock) {
  const temporary = await mkdtemp(join(cache, ".install-"));
  const target = join(temporary, "site-packages");
  try {
    const requirements = join(temporary, "requirements.txt");
    await writeFile(requirements, lock.pythonWheels.map((wheel) =>
      `${pathToFileURL(join(cache, wheel.fileName)).href} --hash=sha256:${wheel.sha256}`).join("\n") + "\n");
    pip(["install", "--no-index", "--no-deps", "--no-cache-dir", "--no-compile", "--ignore-installed",
      "--only-binary=:all:", "--require-hashes", "--target", target, "-r", requirements]);
    const binaries = join(target, "imageio_ffmpeg/binaries");
    for (const name of await readdir(binaries)) {
      if (name.startsWith("ffmpeg-")) await chmod(join(binaries, name), 0o755);
    }
    smoke(target, lock);
    await mkdir(sitePackages, { recursive: true });
    if (await realpath(sitePackages) !== sitePackages) throw new Error("App site-packages must not be a symlink");
    // Preserve portable Python's pip while replacing only the vendored package entries.
    const names = new Set(lock.pythonWheels.map((wheel) => wheel.name));
    for (const name of await readdir(sitePackages)) {
      if (name.endsWith(".dist-info") && names.has(normalize(name.replace(/-[^-]+\.dist-info$/, "")))) {
        await rm(join(sitePackages, name), { recursive: true, force: true });
      }
    }
    for (const name of await readdir(target)) {
      await rm(join(sitePackages, name), { recursive: true, force: true });
      await rename(join(target, name), join(sitePackages, name));
    }
    smoke(sitePackages, lock);
  } finally {
    await rm(temporary, { recursive: true, force: true });
  }
}

async function bytes(directory) {
  let total = 0;
  for (const item of await readdir(directory, { withFileTypes: true })) {
    const path = join(directory, item.name);
    if (item.isDirectory()) total += await bytes(path);
    else if (item.isFile()) total += (await lstat(path)).size;
  }
  return total;
}

export async function stageMacosPython({ refreshLock: refresh = false, offline = false } = {}) {
  if (refresh && offline) throw new Error("Cannot refresh the lock offline");
  if (process.platform !== "darwin" || process.arch !== "arm64") {
    throw new Error("macOS Python staging requires an Apple Silicon macOS host");
  }
  runPython(["-c", "import platform, sys; assert sys.version_info[:3] == (3, 12, 12); assert platform.machine() == 'arm64'"]);
  let lock;
  if (refresh) lock = await refreshLock();
  else {
    try {
      lock = JSON.parse(await readFile(lockPath, "utf8"));
    } catch (error) {
      if (error.code === "ENOENT") throw new Error("Mac lock missing; run with --refresh-lock explicitly to generate it");
      throw error;
    }
  }
  validateLock(lock);
  for (const wheel of lock.pythonWheels) await ensureWheel(wheel, cache, { offline });
  console.log(runPython(["-c", verifyWheels, cache], { input: JSON.stringify(lock) }).trim());
  await install(lock);
  if (refresh) {
    const temporary = `${lockPath}.tmp`;
    await writeFile(temporary, JSON.stringify(lock, null, 2) + "\n");
    await rename(temporary, lockPath);
    console.log(`Refreshed ${lockPath}`);
  }
  const sizes = { wheelCacheBytes: await bytes(cache), sitePackagesBytes: await bytes(sitePackages), pythonRuntimeBytes: await bytes(pythonRoot) };
  console.log(`Staged ${lock.pythonWheels.length} Harmony Cut wheels into ${sitePackages}`);
  console.log(JSON.stringify(sizes));
  return sizes;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    const options = parseArgs(process.argv.slice(2));
    if (options.help) console.log("Usage: node scripts/stage-macos-python.mjs [--offline | --refresh-lock]\nDefault: verify the committed lock and cache; fetch only missing locked wheels.\n--offline: fail on missing cache entries, never access the network.\n--refresh-lock: resolve exact Windows pins on PyPI and explicitly regenerate the Mac lock after smoke checks.");
    else await stageMacosPython(options);
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  }
}
