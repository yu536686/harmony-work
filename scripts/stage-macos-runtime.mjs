import { build } from "esbuild";
import { chmod, copyFile, mkdir, readFile, writeFile } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const resources = resolve(root, "src-tauri", "resources");
const PINNED_NODE_VERSION = "v24.18.0";

export async function stageMacosRuntime() {
  if (process.platform !== "darwin" || Number(process.versions.node.split(".")[0]) < 22) {
    throw new Error("macOS desktop automation requires macOS and Node.js 22 or newer");
  }
  if (process.version !== PINNED_NODE_VERSION) {
    throw new Error(`macOS bundle requires Node ${PINNED_NODE_VERSION}; found ${process.version}`);
  }

  const packageRoot = resolve(root, "node_modules", "mac-use-mcp");
  const packageJson = JSON.parse(await readFile(resolve(packageRoot, "package.json"), "utf8"));
  const automationRoot = resolve(resources, "mac-use-mcp");
  const cli = resolve(automationRoot, "dist", "helpers", "cli.mjs");
  const helper = resolve(automationRoot, "dist", "bin", "input-helper");
  await mkdir(dirname(cli), { recursive: true });
  await mkdir(dirname(helper), { recursive: true });
  await build({
    entryPoints: [resolve(packageRoot, "dist", "index.js")],
    outfile: cli,
    bundle: true,
    platform: "node",
    format: "esm",
    target: "node22",
    logLevel: "warning",
  });
  await copyFile(resolve(packageRoot, "dist", "bin", "input-helper"), helper);
  await chmod(helper, 0o755);
  await copyFile(resolve(packageRoot, "LICENSE"), resolve(automationRoot, "LICENSE"));
  await writeFile(resolve(automationRoot, "dist", "package.json"), JSON.stringify({ version: packageJson.version }), "utf8");

  const staged = spawnSync("zsh", [resolve(root, "scripts", "stage-macos-offline-runtime.sh")], {
    stdio: "inherit",
  });
  if (staged.status !== 0) {
    throw new Error(`macOS offline runtime staging failed (exit ${staged.status})`);
  }
  console.log(`Staged mac-use-mcp ${packageJson.version}`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  stageMacosRuntime().catch((error) => {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  });
}
