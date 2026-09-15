import assert from "node:assert/strict";
import { readdir, readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const projectRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const sourceExtensions = new Set([".ts", ".tsx", ".js", ".jsx"]);

async function collectFiles(relativePath) {
  const absolutePath = path.join(projectRoot, relativePath);
  const entries = await readdir(absolutePath, { withFileTypes: true });
  const files = [];

  for (const entry of entries) {
    const childRelativePath = path.join(relativePath, entry.name);
    if (entry.isDirectory()) {
      files.push(...(await collectFiles(childRelativePath)));
    } else if (
      sourceExtensions.has(path.extname(entry.name)) &&
      !entry.name.includes(".test.") &&
      !entry.name.includes(".spec.")
    ) {
      files.push(childRelativePath);
    }
  }

  return files;
}

async function findLegacyBranding() {
  const files = [
    "src/App.tsx",
    ...(await collectFiles("src/components")),
    ...(await collectFiles("src/i18n")),
    "index.html",
    "src-tauri/tauri.conf.json",
    "src-tauri/tauri.windows.conf.json",
    "src-tauri/src/app/bootstrap.rs",
    "src-tauri/src/commands/platform/window.rs",
    "src-tauri/src/codex_runtime/background.rs",
    "src-tauri/src/codex_runtime/service.rs",
    "installer/WorkBuddyBootstrapper/MainWindow.xaml",
    "installer/WorkBuddyBootstrapper/MainWindow.xaml.cs",
    "installer/WorkBuddyBootstrapper/InstallerEngine.cs",
    "installer/WorkBuddyBootstrapper/WorkBuddyBootstrapper.csproj",
    "README.md",
  ];
  const forbidden = [
    /小亨\s*WorkBuddy/g,
    /Xiaoheng\s*WorkBuddy/g,
    /Harmony\s*WorkBuddy/g,
    /github\.com\/yu536686\/xiaoheng-workbuddy/gi,
    /Xiaoheng-WorkBuddy-(?:Setup|<version>)/g,
    /"mainBinaryName"\s*:\s*"xiaoheng-workbuddy"/g,
    /src=["']\/piscis\.png["']/g,
  ];
  const matches = [];

  for (const relativePath of files) {
    const contents = await readFile(path.join(projectRoot, relativePath), "utf8");
    for (const pattern of forbidden) {
      pattern.lastIndex = 0;
      for (const match of contents.matchAll(pattern)) {
        const line = contents.slice(0, match.index).split(/\r?\n/).length;
        const sourceLine = contents.split(/\r?\n/)[line - 1] ?? "";
        if (relativePath.endsWith("InstallerEngine.cs") && sourceLine.includes("Legacy")) {
          continue;
        }
        matches.push(`${relativePath}:${line}: ${match[0]}`);
      }
    }
  }

  return matches;
}

test("user-visible surfaces use the Harmony Work product brand", async () => {
  const matches = await findLegacyBranding();
  assert.deepEqual(matches, [], `legacy product branding remains:\n${matches.join("\n")}`);
});
