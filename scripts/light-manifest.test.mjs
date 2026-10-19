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
  await assert.rejects(generate(...args,'1.1.1',join(dir,'payload')),/New build dependencies differ/);
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
 assert.ok(source.includes("$ack.version -ne $manifest.version"));
 assert.ok(!source.includes('Remove-Item -LiteralPath $InstallDirectory'));
 const nsi=await readFile(new URL('../installer/light/overlay.nsi',import.meta.url),'utf8');assert.ok(!nsi.includes('WriteUninstaller'));assert.ok(!nsi.includes('ExecWait'));
});
import {stableRuntimePath, inventory} from './light-manifest.mjs';
test('managed runtime compatibility ignores markers, caches, tools but hashes node',async()=>{
 const dir=await mkdtemp(join(tmpdir(),'light-runtime-'));
 async function put(p,text){const f=join(dir,p);await mkdir(dirname(f),{recursive:true});await writeFile(f,text)}
 try {
  await put('node/node.exe','stable node');await put('python/python.exe','stable python');await put('managed-runtime.json','{"installedAt":"yesterday"}');await put('python/Lib/site-packages/__pycache__/module.pyc','cache1');await put('tools/Harmony-Work-Uninstaller.exe','uninstaller1');
  const a=await inventory(dir,stableRuntimePath);
  await put('managed-runtime.json','{"installedAt":"today"}');await put('python/Lib/site-packages/__pycache__/module.pyc','cache2');await put('tools/Harmony-Work-Uninstaller.exe','uninstaller2');
  assert.deepEqual(await inventory(dir,stableRuntimePath),a);
  await put('node/node.exe','changed node');
  const b=await inventory(dir,stableRuntimePath);assert.notEqual(a.find(f=>f.path==='node/node.exe').sha256,b.find(f=>f.path==='node/node.exe').sha256);
  assert.equal(b.some(f=>f.path.includes('pyc')||f.path.startsWith('tools/')||f.path==='managed-runtime.json'),false);
 }finally{await rm(dir,{recursive:true,force:true})}
});
test('license directory optional and updater uses verified WebView2 GUID and dry-run',async()=>{
 const source=await readFile(new URL('../installer/light/update.ps1',import.meta.url),'utf8');
 assert.ok(source.includes('{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'));
 assert.ok(source.includes('if($Check){exit 0}'));
 assert.ok(source.indexOf('if($Check){exit 0}')<source.indexOf('RecoverTransactions $actual'));
 assert.ok(source.indexOf("WriteJournal 'pending';New-Item")<source.lastIndexOf(' -Destination $target -Force}'));
 assert.ok(source.includes("WriteJournal 'committed'"));
 const nsi=await readFile(new URL('../installer/light/overlay.nsi',import.meta.url),'utf8');assert.ok(nsi.includes('"/CHECK"'));
});
test('Python extension and Git support hashes detect changes while caches/config are ignored',async()=>{
 const dir=await mkdtemp(join(tmpdir(),'light-support-'));
 async function put(p,text){const f=join(dir,p);await mkdir(dirname(f),{recursive:true});await writeFile(f,text)}
 try {
  for(const p of ['python/Lib/site-packages/cv2/cv2.pyd','python/Lib/site-packages/numpy/__init__.py','git/mingw64/bin/libcrypto-3-x64.dll','git/mingw64/libexec/git-core/git-remote-https.exe','git/usr/bin/sh.exe'])await put(p,'original');
  await put('python/Lib/site-packages/cv2/__pycache__/x.pyc','cache');await put('git/etc/gitconfig','config');
  const a=await inventory(dir,stableRuntimePath);assert.equal(a.length,5);
  await put('python/Lib/site-packages/cv2/cv2.pyd','changed');await put('git/mingw64/bin/libcrypto-3-x64.dll','changed');
  const b=await inventory(dir,stableRuntimePath);
  for(const p of ['python/Lib/site-packages/cv2/cv2.pyd','git/mingw64/bin/libcrypto-3-x64.dll'])assert.notEqual(a.find(f=>f.path===p).sha256,b.find(f=>f.path===p).sha256);
  await put('python/Lib/site-packages/cv2/__pycache__/x.pyc','new cache');await put('git/etc/gitconfig','new config');assert.deepEqual(await inventory(dir,stableRuntimePath),b);
  const source=await readFile(new URL('../installer/light/update.ps1',import.meta.url),'utf8');assert.ok(source.indexOf('SmokeRuntime $runtimeRoot')<source.indexOf('if($Check){exit 0}'));assert.ok(source.includes('import cv2,numpy,starlette,uvicorn,pydantic,imageio_ffmpeg'));
 }finally{await rm(dir,{recursive:true,force:true})}
});
test('preflight probes write permissions and failure restores validated prior app',async()=>{
 const source=await readFile(new URL('../installer/light/update.ps1',import.meta.url),'utf8');
 assert.ok(source.indexOf('ProbeInstallWrite $actual')<source.indexOf('if($Check){exit 0}'));
 assert.ok(source.includes('[IO.FileMode]::CreateNew'));assert.ok(source.includes('$stream.Flush($true)'));
 assert.ok(source.includes(".VersionInfo.ProductName -eq 'Harmony Work'"));
 assert.ok(source.includes('if(!$Check -and $canRestart -and !$rollbackErrors.Count -and !$recoveryFailed)'));
 assert.ok(source.includes('if(!$running){Start-Process -FilePath $originalExe'));
 assert.ok(source.includes('catch {continue} # A partially published ack'));
 assert.ok(source.includes("throw 'Updated application health acknowledgement does not match this transaction'"));
});
