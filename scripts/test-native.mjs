import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const tauriConfig = {
  bundle: { resources: [] },
};

// Unit tests do not stage installer payloads or activate macOS-only window effects.
if (process.platform === "darwin") {
  tauriConfig.app = { macOSPrivateApi: true };
}

const result = spawnSync("cargo", ["test", "--locked", "--lib"], {
  cwd: path.join(root, "src-tauri"),
  env: { ...process.env, TAURI_CONFIG: JSON.stringify(tauriConfig) },
  stdio: "inherit",
});

if (result.error) {
  throw result.error;
}
process.exit(result.status ?? 1);
