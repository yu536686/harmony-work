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

  const nodeRoot = resolve(resources, "node");
  const nodeBinary = resolve(nodeRoot, "bin", "node");
  await mkdir(dirname(nodeBinary), { recursive: true });
  await copyFile(process.execPath, nodeBinary);
  await chmod(nodeBinary, 0o755);
  const version = spawnSync(nodeBinary, ["--version"], { encoding: "utf8" });
  if (version.status !== 0 || version.stdout.trim() !== process.version) {
    throw new Error(`Bundled Node verification failed: ${version.stderr || version.stdout}`);
  }
  await copyFile(
    resolve(root, "src-tauri", "licenses", "node-v24.18.0-LICENSE"),
    resolve(nodeRoot, "LICENSE"),
  );
  await writeFile(resolve(nodeRoot, "VERSION"), `${process.version}\n`, "utf8");
  console.log(`Staged Node ${process.version} and mac-use-mcp ${packageJson.version}`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  stageMacosRuntime().catch((error) => {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  });
}
