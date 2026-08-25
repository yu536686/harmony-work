import { chmod, copyFile, mkdir } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");

export function mcpHostBinaryName(platform = process.platform) {
  return platform === "win32" ? "workbuddy-mcp-host.exe" : "workbuddy-mcp-host";
}

export function mcpHostPaths({
  platform = process.platform,
  targetTriple = process.env.TAURI_ENV_TARGET_TRIPLE || null,
} = {}) {
  const binary = mcpHostBinaryName(platform);
  const targetDirectory = targetTriple
    ? resolve(root, "src-tauri", "target", targetTriple, "release")
    : resolve(root, "src-tauri", "target", "release");
  return {
    source: resolve(targetDirectory, binary),
    destination: resolve(
      root,
      "src-tauri",
      "resources",
      "workbuddy-mcp-host",
      binary,
    ),
  };
}

export async function stageWorkBuddyMcpHost() {
  const targetTriple = process.env.TAURI_ENV_TARGET_TRIPLE || null;
  const cargoArgs = [
    "build",
    "--manifest-path",
    resolve(root, "src-tauri", "Cargo.toml"),
    "-p",
    "workbuddy-mcp-host",
    "--release",
  ];
  if (targetTriple) cargoArgs.push("--target", targetTriple);
  const build = spawnSync(process.env.CARGO || "cargo", cargoArgs, {
    cwd: root,
    encoding: "utf8",
  });
  if (build.status !== 0) {
    throw new Error(
      `WorkBuddy MCP Host build failed:\n${build.stdout || ""}${build.stderr || ""}`,
    );
  }
  const { source, destination } = mcpHostPaths({
    platform: process.platform,
    targetTriple,
  });
  await mkdir(dirname(destination), { recursive: true });
  await copyFile(source, destination);
  if (process.platform !== "win32") await chmod(destination, 0o755);
  console.log(`Staged WorkBuddy MCP Host: ${destination}`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  stageWorkBuddyMcpHost().catch((error) => {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  });
}
