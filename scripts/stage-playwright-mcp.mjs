import { cp, mkdir, rm, stat } from 'node:fs/promises';
import path from 'node:path';

const packages = ['@playwright/mcp', 'playwright', 'playwright-core'];

export async function stagePlaywrightMcp(modules, destination) {
  for (const name of packages) {
    const source = path.join(modules, name);
    if (!(await stat(source).catch(() => null))?.isDirectory()) {
      throw new Error(`Missing Playwright MCP package: ${name}. Run npm ci before building.`);
    }
  }
  await rm(destination, { recursive: true, force: true });
  await mkdir(path.join(destination, 'node_modules'), { recursive: true });
  await cp(path.join(modules, '@playwright', 'mcp'), destination, { recursive: true });
  for (const name of packages.slice(1)) {
    await cp(path.join(modules, name), path.join(destination, 'node_modules', name), {
      recursive: true,
    });
  }
}

if (process.argv[1]?.endsWith('stage-playwright-mcp.mjs')) {
  const root = path.resolve(import.meta.dirname, '..');
  await stagePlaywrightMcp(
    path.join(root, 'node_modules'),
    path.join(root, 'src-tauri', 'resources', 'playwright-mcp'),
  );
}
