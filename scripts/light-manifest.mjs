import { readdir, lstat, readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
import { pathToFileURL } from 'node:url';
export function safePath(p) { return typeof p === 'string' && /^[a-zA-Z0-9_. /@+\[\]()-]+$/.test(p) && !p.startsWith('/') && p.split('/').every(x => x && x !== '.' && x !== '..' && !x.endsWith('.') && !x.endsWith(' ')); }
export function owned(p) { return p === 'harmony-work.exe' || p.startsWith('resources/workbuddy-mcp-host/') || p.startsWith('resources/harmony-cut/') || p.startsWith('licenses/') || p === 'resources/artifact-runtime/THIRD_PARTY_NOTICES.txt'; }
export function stableRuntimePath(p) {
 if(!stableResourcePath(p)) return false;
 if(/^python\/(?:Lib|libs|DLLs)\/.*\.(?:py|pyd|dll|zip)$/i.test(p)) return true;
 if(/^python\/[^/]+\.(?:dll|pyd|zip)$/i.test(p)) return true;
 // Only immutable Git executable/support directories, never etc/config or user state.
 if(/^git\/(?:bin|cmd|mingw64\/(?:bin|libexec\/git-core)|usr\/(?:bin|libexec|lib\/perl5))\//i.test(p) && ( /\.(?:exe|dll|sh|pl|pm|py)$/i.test(p) || !p.split('/').at(-1).includes('.') )) return true;
 return /^(node\/(node\.exe|[^/]+\.dll)|python\/(python(?:3|312)?\.(?:exe|dll)|python312\.zip|python312\._pth)|git\/(?:bin\/git\.exe|cmd\/git\.exe)|wecom\/(?:wecom-cli\.cmd|node_modules\/@wecom\/cli\/(?:package\.json|bin\/wecom\.js)))$/.test(p);
}
export function stableResourcePath(p) { return !/(^|\/)(__pycache__|\.cache|cache|logs|tmp|temp)(\/|$)/i.test(p) && !/\.(pyc|pyo|log)$/i.test(p); }
export async function inventory(root, accept = () => true) {
 const out=[];
 async function walk(dir,prefix='') { for(const name of (await readdir(dir)).sort()) { const p=prefix+name; if(!safePath(p)) throw Error('Unsafe path '+p); const s=await lstat(join(dir,name)); if(s.isSymbolicLink()) throw Error('Symlink '+p); if(s.isDirectory()) await walk(join(dir,name),p+'/'); else if(s.isFile() && accept(p)) out.push({path:p,size:s.size,sha256:createHash('sha256').update(await readFile(join(dir,name))).digest('hex')}); } }
 await walk(root); return out.sort((a,b)=>a.path.localeCompare(b.path,'en'));
}
function pythonLayout(files) {
 const paths=new Set(files.map(f=>f.path));
 if(paths.has('python/python312.zip')&&paths.has('python/python312._pth')) return 'python-embedded';
 if(['python/Lib/os.py','python/Lib/encodings/__init__.py','python/DLLs/_ssl.pyd'].every(p=>paths.has(p))) return 'python-standard';
 throw Error('Unsupported Python layout in runtime compatibility reference');
}
export async function generate(payload, baseline, managed, version, expectedDependencies, compatibleManagedRuntime) {
 if(!/^1\.1\.(1|2(?:-rc\.[1-9]\d*)?)$/.test(version)) throw Error('Unsupported version');
 const files=await inventory(payload); if(!files.some(f=>f.path==='harmony-work.exe') || files.some(f=>!owned(f.path))) throw Error('Invalid payload ownership');
 for(const prefix of ['resources/workbuddy-mcp-host/','resources/harmony-cut/']) if(!files.some(f=>f.path.startsWith(prefix))) throw Error('Missing payload '+prefix);
 const dependencies=await inventory(baseline,p=>!owned(p)&&p.startsWith('resources/')&&!p.startsWith('resources/offline-installers/')&&stableResourcePath(p));
 for(const name of ['codex','flaui-mcp','playwright-mcp']) if(!dependencies.some(f=>f.path.startsWith('resources/'+name+'/'))) throw Error('Missing baseline '+name);
 if(expectedDependencies) { const expected=await inventory(expectedDependencies,p=>!owned(p)&&p.startsWith('resources/')&&!p.startsWith('resources/offline-installers/')&&stableResourcePath(p)); if(JSON.stringify(expected)!==JSON.stringify(dependencies)) throw Error('New build dependencies differ from baseline; a full installer is required'); }
 const managedDependencies=await inventory(managed,stableRuntimePath);
 for(const name of ['node/node.exe','python/python.exe','git/bin/git.exe','wecom/wecom-cli.cmd']) if(!managedDependencies.some(f=>f.path===name)) throw Error('Missing runtime '+name);
 let managedRuntimeProfiles;
 if(compatibleManagedRuntime) {
  const alternate=await inventory(compatibleManagedRuntime,stableRuntimePath);
  const originalCommon=managedDependencies.filter(f=>!f.path.startsWith('python/'));
  if(JSON.stringify(originalCommon)!==JSON.stringify(alternate.filter(f=>!f.path.startsWith('python/')))) throw Error('Common runtime differs between compatibility references');
  const lookup=new Map(alternate.map(f=>[f.path,f]));
  for(const f of managedDependencies.filter(f=>/^python\/(?:python\.exe|python3(?:12)?\.dll|Lib\/site-packages\/)/.test(f.path))) {
   if(JSON.stringify(f)!==JSON.stringify(lookup.get(f.path))) throw Error('Python dependency differs between compatibility references: '+f.path);
  }
  const primaryId=pythonLayout(managedDependencies),alternateId=pythonLayout(alternate);
  if(primaryId===alternateId) throw Error('Compatibility references must use different Python layouts');
  managedRuntimeProfiles=[{id:primaryId,files:managedDependencies},{id:alternateId,files:alternate}];
 }
 return {schemaVersion:1,version,mainExe:'harmony-work.exe',files,dependencies,managedDependencies,...(managedRuntimeProfiles?{managedRuntimeProfiles}:{})};
}
if(process.argv[1]&&import.meta.url===pathToFileURL(process.argv[1]).href){const [payload,baseline,managed,version,output,expected,compatible]=process.argv.slice(2);await writeFile(output,JSON.stringify(await generate(payload,baseline,managed,version,expected,compatible),null,2)+'\n');}
