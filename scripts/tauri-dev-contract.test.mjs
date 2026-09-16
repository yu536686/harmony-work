import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const projectRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

test("Tauri dev URL and Vite dev script use the same port", async () => {
  const [packageJson, tauriConfig, viteConfig] = await Promise.all([
    readFile(path.join(projectRoot, "package.json"), "utf8"),
    readFile(path.join(projectRoot, "src-tauri", "tauri.conf.json"), "utf8"),
    readFile(path.join(projectRoot, "vite.config.ts"), "utf8"),
  ]);
  const packageConfig = JSON.parse(packageJson);
  const tauri = JSON.parse(tauriConfig);
  const port = new URL(tauri.build.devUrl).port;

  assert.ok(port, "Tauri devUrl must declare a port");
  assert.match(
    packageConfig.scripts.dev,
    new RegExp(`(?:--port\\s+|--port=)${port}(?:\\s|$)`),
    "the Vite dev script must listen on Tauri's devUrl port",
  );
  assert.match(
    viteConfig,
    /ignored:\s*\[[^\]]*"\*\*\/target\/\*\*"/s,
    "Vite must ignore Rust target artifacts so the locked desktop executable cannot crash HMR",
  );
});
