import {test} from 'node:test';
import assert from 'node:assert/strict';
import {safePath,owned} from './light-manifest.mjs';
test('payload ownership preserves third party resources and uninstallers',()=>{for(const p of ['uninstall.exe','Harmony-Work-Uninstaller.exe','resources/codex/codex.exe','resources/flaui-mcp/a.dll','resources/playwright-mcp/cli.js','runtime/node.exe'])assert.equal(owned(p),false);for(const p of ['harmony-work.exe','resources/workbuddy-mcp-host/index.js','resources/harmony-cut/server.py','licenses/NOTICE'])assert.equal(owned(p),true)});
test('reject path traversal and Windows stream or absolute paths',()=>{for(const p of ['../x','/x','C:/x','x/../y','x\\y','x:y','x.','x /y'])assert.equal(safePath(p),false,p);assert.equal(safePath('resources/harmony-cut/server.py'),true)});
import {mkdtemp,mkdir,writeFile,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join,dirname} from 'node:path';
import {generate} from './light-manifest.mjs';
test('manifest deterministic with exact compatibility hashes, missing dependency fails',async()=>{
 const dir=await mkdtemp(join(tmpdir(),'light-manifest-'));
 async function put(root,p,text='content'){const file=join(dir,root,p);await mkdir(dirname(file),{recursive:true});await writeFile(file,text)}
 try{
  for(const p of ['harmony-work.exe','resources/workbuddy-mcp-host/index.js','resources/harmony-cut/server.py','licenses/NOTICE'])await put('payload',p);
  for(const p of ['resources/codex/codex.exe','resources/flaui-mcp/mcp.exe','resources/playwright-mcp/cli.js'])await put('baseline',p);
  for(const p of ['node/node.exe','python/python.exe','git/bin/git.exe','wecom/wecom-cli.cmd'])await put('managed',p);
  const args=['payload','baseline','managed'].map(x=>join(dir,x));
  const a=await generate(...args,'1.1.1');assert.deepEqual(a,await generate(...args,'1.1.1'));assert.equal(a.dependencies.length,3);assert.equal(a.files.length,4);assert.equal(a.files[0].sha256.length,64);
  await put('baseline','resources/codex/codex.exe','changed');assert.notEqual((await generate(...args,'1.1.1')).dependencies[0].sha256,a.dependencies[0].sha256);
  await rm(join(dir,'baseline/resources/flaui-mcp'),{recursive:true});await assert.rejects(generate(...args,'1.1.1'),/Missing baseline flaui-mcp/);
  await assert.rejects(generate(...args,'9.0.0'),/Unsupported version/);
 }finally{await rm(dir,{recursive:true,force:true})}
});
import {readFile} from 'node:fs/promises';
test('transaction validates dependencies before overlay and retains rollback backups',async()=>{
 const source=await readFile(new URL('../installer/light/update.ps1',import.meta.url),'utf8');
 assert.ok(source.indexOf('Verify $actual $manifest.dependencies')<source.indexOf('$transaction=Join-Path'));
 assert.ok(source.includes('[array]::Reverse($changed)'));
 assert.ok(source.includes('HARMONY_UPDATE_HEALTH_TOKEN'));
 assert.ok(source.includes("$ack.version -eq $manifest.version"));
 assert.ok(!source.includes('Remove-Item -LiteralPath $InstallDirectory'));
 const nsi=await readFile(new URL('../installer/light/overlay.nsi',import.meta.url),'utf8');assert.ok(!nsi.includes('WriteUninstaller'));assert.ok(!nsi.includes('ExecWait'));
});
