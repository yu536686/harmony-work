import { readdir, lstat, readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
import { pathToFileURL } from 'node:url';
export function safePath(p) { return typeof p === 'string' && /^[a-zA-Z0-9_. /@+-]+$/.test(p) && !p.startsWith('/') && p.split('/').every(x => x && x !== '.' && x !== '..' && !x.endsWith('.') && !x.endsWith(' ')); }
export function owned(p) { return p === 'harmony-work.exe' || p.startsWith('resources/workbuddy-mcp-host/') || p.startsWith('resources/harmony-cut/') || p.startsWith('licenses/') || p === 'resources/artifact-runtime/THIRD_PARTY_NOTICES.txt'; }
export async function inventory(root) {
 const out=[];
 async function walk(dir,prefix='') { for(const name of (await readdir(dir)).sort()) { const p=prefix+name; if(!safePath(p)) throw Error('Unsafe path '+p); const s=await lstat(join(dir,name)); if(s.isSymbolicLink()) throw Error('Symlink '+p); if(s.isDirectory()) await walk(join(dir,name),p+'/'); else if(s.isFile()) out.push({path:p,size:s.size,sha256:createHash('sha256').update(await readFile(join(dir,name))).digest('hex')}); } }
 await walk(root); return out.sort((a,b)=>a.path.localeCompare(b.path,'en'));
}
export async function generate(payload, baseline, managed, version, expectedDependencies) {
 if(!/^1\.1\.(1|2-rc\.1)$/.test(version)) throw Error('Unsupported version');
 const files=await inventory(payload); if(!files.some(f=>f.path==='harmony-work.exe') || files.some(f=>!owned(f.path))) throw Error('Invalid payload ownership');
 for(const prefix of ['resources/workbuddy-mcp-host/','resources/harmony-cut/','licenses/']) if(!files.some(f=>f.path.startsWith(prefix))) throw Error('Missing payload '+prefix);
 const dependencies=(await inventory(baseline)).filter(f=>!owned(f.path)&& f.path.startsWith('resources/') && !f.path.startsWith('resources/offline-installers/'));
 for(const name of ['codex','flaui-mcp','playwright-mcp']) if(!dependencies.some(f=>f.path.startsWith('resources/'+name+'/'))) throw Error('Missing baseline '+name);
 if(expectedDependencies) { const expected=(await inventory(expectedDependencies)).filter(f=>!owned(f.path)&&f.path.startsWith('resources/')&&!f.path.startsWith('resources/offline-installers/')); if(JSON.stringify(expected)!==JSON.stringify(dependencies)) throw Error('New build dependencies differ from baseline; a full installer is required'); }
 const managedDependencies=await inventory(managed);
 for(const name of ['node/node.exe','python/python.exe','git/bin/git.exe','wecom/wecom-cli.cmd']) if(!managedDependencies.some(f=>f.path===name)) throw Error('Missing runtime '+name);
 return {schemaVersion:1,version,mainExe:'harmony-work.exe',files,dependencies,managedDependencies};
}
if(process.argv[1]&&import.meta.url===pathToFileURL(process.argv[1]).href){const [payload,baseline,managed,version,output,expected]=process.argv.slice(2);await writeFile(output,JSON.stringify(await generate(payload,baseline,managed,version,expected),null,2)+'\n');}
