import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import test from 'node:test';

const root = path.resolve(import.meta.dirname, '..');
const source = file => readFile(path.join(root, file), 'utf8');

test('desktop crate no longer depends on RobotZ', async () => {
  const cargo = await source('src-tauri/Cargo.toml');
  assert.doesNotMatch(cargo, /robotz-(?:browser|automation|core)/i);
  const store = await source('src-tauri/src/store/mod.rs');
  assert.doesNotMatch(store, /robotz_browser|browser:.*SharedBrowserManager/);
  const bootstrap = await source('src-tauri/src/app/bootstrap.rs');
  assert.doesNotMatch(bootstrap, /robotz_automation|browser: browser\.clone\(\)/);
});

test('automation servers are packaged resources, not first-run downloads', async () => {
  const tauri = JSON.parse(await source('src-tauri/tauri.conf.json'));
  assert.ok(tauri.bundle.resources.includes('resources/playwright-mcp'));
  assert.ok(tauri.bundle.resources.includes('resources/flaui-mcp'));
  const config = await source('src-tauri/src/codex_runtime/config.rs');
  assert.match(config, /\[mcp_servers\.playwright\]/);
  assert.match(config, /\[mcp_servers\.windows_desktop\]/);
  assert.doesNotMatch(config, /npx @playwright\/mcp/);
});
