import { chmod, copyFile, mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { spawnSync } from "node:child_process";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const TARGETS = {
  "darwin-arm64": ["darwin-arm64", "aarch64-apple-darwin", "codex"],
  "darwin-x64": ["darwin-x64", "x86_64-apple-darwin", "codex"],
  "win32-x64": ["win32-x64", "x86_64-pc-windows-msvc", "codex.exe"],
  "win32-arm64": ["win32-arm64", "aarch64-pc-windows-msvc", "codex.exe"],
  "linux-x64": ["linux-x64", "x86_64-unknown-linux-musl", "codex"],
  "linux-arm64": ["linux-arm64", "aarch64-unknown-linux-musl", "codex"],
};

export function codexTarget(platform = process.platform, arch = process.arch) {
  const target = TARGETS[`${platform}-${arch}`];
  if (!target) throw new Error(`Unsupported Codex sidecar target: ${platform}-${arch}`);
  return target;
}

export function codexSidecarPaths(platform = process.platform, arch = process.arch) {
  const [packageSuffix, targetTriple, binary] = codexTarget(platform, arch);
  return {
    source: resolve(
      root,
      "node_modules",
      "@openai",
      `codex-${packageSuffix}`,
      "vendor",
      targetTriple,
      "bin",
      binary,
    ),
    destination: resolve(root, "src-tauri", "resources", "codex", binary),
  };
}

export async function stageCodexSidecar() {
  const { source, destination } = codexSidecarPaths();
  const packageJson = JSON.parse(
    await readFile(resolve(root, "node_modules", "@openai", "codex-sdk", "package.json"), "utf8"),
  );
  await mkdir(dirname(destination), { recursive: true });
  await copyFile(source, destination);
  if (process.platform !== "win32") await chmod(destination, 0o755);

  const version = spawnSync(destination, ["--version"], { encoding: "utf8" });
  if (version.status !== 0 || !version.stdout.includes(packageJson.version)) {
    throw new Error(
      `Staged Codex version check failed: ${version.stdout || version.stderr || "no output"}`,
    );
  }
  await writeFile(
    resolve(dirname(destination), "VERSION"),
    `${packageJson.version}\n`,
    "utf8",
  );
  console.log(`Staged Codex ${packageJson.version}: ${destination}`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  stageCodexSidecar().catch((error) => {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  });
}
