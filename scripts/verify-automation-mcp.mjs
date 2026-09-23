import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import { createServer } from 'node:http';
import path from 'node:path';

async function probe(command, args, expectedTool, exercise) {
  const child = spawn(command, args, { windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] });
  let stderr = '';
  child.stderr.setEncoding('utf8');
  child.stderr.on('data', chunk => { stderr += chunk; });
  const lines = createInterface({ input: child.stdout });
  const responses = new Map();
  const waiters = new Map();
  lines.on('line', line => {
    let message;
    try { message = JSON.parse(line); } catch { return; }
    if (message.id == null) return;
    responses.set(message.id, message);
    waiters.get(message.id)?.(message);
  });
  const request = (id, method, params) => {
    child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', id, method, params })}\n`);
    return new Promise((resolve, reject) => {
      if (responses.has(id)) return resolve(responses.get(id));
      const timer = setTimeout(() => reject(new Error(`${method} timed out: ${stderr}`)), 60000);
      waiters.set(id, message => { clearTimeout(timer); resolve(message); });
    });
  };
  try {
    const initialized = await request(1, 'initialize', {
      protocolVersion: '2025-06-18',
      capabilities: {},
      clientInfo: { name: 'harmony-release-probe', version: '1.0' },
    });
    assert.ok(initialized.result, `${command} initialize failed: ${JSON.stringify(initialized)}`);
    child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' })}\n`);
    const listed = await request(2, 'tools/list', {});
    const tools = listed.result?.tools ?? [];
    assert.ok(tools.some(tool => tool.name === expectedTool),
      `${command} missing ${expectedTool}: ${JSON.stringify(listed)}`);
    console.log(`${path.basename(command)}: ${tools.length} tools, ${expectedTool} ready`);
    if (exercise) {
      let id = 3;
      const call = async (name, args = {}) => {
        const response = await request(id++, 'tools/call', { name, arguments: args });
        assert.ok(response.result && !response.result.isError,
          `${name} failed: ${JSON.stringify(response)}`);
        return response.result;
      };
      await exercise(call, tools);
    }
  } finally {
    child.stdin.end();
    child.kill();
  }
}

const root = path.resolve(import.meta.dirname, '..');
const server = createServer((_request, response) => {
  response.setHeader('Content-Type', 'text/html; charset=utf-8');
  response.end('<button onclick="document.querySelector(\'#result\').textContent=\'Clicked successfully\'">Run check</button><p id="result">Pending</p>');
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
try {
  const address = server.address();
  const url = `http://127.0.0.1:${address.port}/`;
  await probe(
    process.execPath,
    [path.join(root, 'src-tauri', 'resources', 'playwright-mcp', 'cli.js'), '--browser=msedge', '--headless'],
    'browser_snapshot',
    async call => {
      await call('browser_navigate', { url });
      const snapshot = await call('browser_snapshot');
      const text = snapshot.content.filter(item => item.type === 'text').map(item => item.text).join('\n');
      const reference = text.match(/button "Run check" \[ref=([^\]]+)\]/)?.[1];
      assert.ok(reference, `button reference missing from browser snapshot: ${text}`);
      await call('browser_click', { element: 'Run check button', target: reference });
      const after = await call('browser_snapshot');
      assert.match(after.content.filter(item => item.type === 'text').map(item => item.text).join('\n'), /Clicked successfully/);
      const screenshot = await call('browser_take_screenshot', { scale: 'css' });
      assert.ok(screenshot.content.some(item => item.type === 'image'), 'browser screenshot is missing');
      console.log('Playwright browser navigate/snapshot/click/screenshot passed');
    },
  );
} finally {
  server.close();
}
if (process.platform === 'win32') {
  await probe(
    path.join(root, 'src-tauri', 'resources', 'flaui-mcp', 'FlaUI.Mcp.exe'),
    [],
    'windows_snapshot',
    async (_call, tools) => {
      console.log(`Windows desktop tools: ${tools.map(tool => tool.name).join(', ')}`);
    },
  );
}
