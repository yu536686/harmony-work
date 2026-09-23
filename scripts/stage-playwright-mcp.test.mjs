import assert from 'node:assert/strict';
import { mkdtemp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { stagePlaywrightMcp } from './stage-playwright-mcp.mjs';

test('stages Playwright MCP with its runtime dependencies', async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), 'workbuddy-playwright-stage-'));
  try {
    const modules = path.join(root, 'node_modules');
    for (const [name, file] of [
      ['@playwright/mcp', 'cli.js'],
      ['playwright', 'index.js'],
      ['playwright-core', 'index.js'],
    ]) {
      const directory = path.join(modules, name);
      await mkdir(directory, { recursive: true });
      await writeFile(path.join(directory, file), name);
    }
    const destination = path.join(root, 'resources', 'playwright-mcp');
    await stagePlaywrightMcp(modules, destination);
    assert.equal(await readFile(path.join(destination, 'cli.js'), 'utf8'), '@playwright/mcp');
    assert.equal(
      await readFile(path.join(destination, 'node_modules', 'playwright-core', 'index.js'), 'utf8'),
      'playwright-core',
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('fails instead of staging an incomplete MCP', async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), 'workbuddy-playwright-stage-'));
  try {
    await assert.rejects(
      stagePlaywrightMcp(path.join(root, 'node_modules'), path.join(root, 'output')),
      /@playwright\/mcp/,
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
