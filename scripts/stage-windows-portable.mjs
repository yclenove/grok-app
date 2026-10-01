#!/usr/bin/env node
// Build-time copying/integrity check, not publisher signature verification.
// The release wrapper verifies the seed with the existing Rust runtime checker.
import { createHash, randomUUID } from 'node:crypto';
import { constants, copyFileSync, createReadStream, lstatSync, mkdirSync, openSync, closeSync, readSync, readdirSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { basename, dirname, isAbsolute, join, parse, relative, resolve, sep } from 'node:path';
import { pathToFileURL } from 'node:url';
import { hashFile, relativePath } from './windows-install-inventory.mjs';

const FORMAT='grok-windows-portable-files-v1';
const MANIFEST='portable-files.json';
const TARGET='x86_64-windows';
const MAX_FILES=8192;
const expectedResources=[
  'resources/computer-use/windows-x64.lock.json',
  'resources/computer-use/pack-targets.json',
  'resources/computer-use/README.md',
  'resources/computer-use/seed/manifest.json',
  'resources/computer-use/seed/bin/*',
  'resources/computer-use/seed/playwright/**/*',
  'resources/computer-use/seed/chromium/chrome-win/**/*',
];
const inside=(root,file)=>{const value=relative(resolve(root),resolve(file));return !isAbsolute(value)&&value!=='..'&&!value.startsWith('..'+sep);};
const entryExists=file=>lstatSync(file,{throwIfNoEntry:false})!==undefined;
function plainDirectory(dir) {
  const absolute=resolve(dir),base=parse(absolute).root;let current=base;
  for(const part of absolute.slice(base.length).split(/[\\/]/).filter(Boolean)) {
    current=join(current,part);const stat=lstatSync(current);
    if(!stat.isDirectory()||stat.isSymbolicLink())throw new Error('portable directory contains a link or non-directory');
  }
}
function makePlainDirectory(dir) {
  if(entryExists(dir)){plainDirectory(dir);return;}
  makePlainDirectory(dirname(resolve(dir)));
  mkdirSync(dir);plainDirectory(dir);
}
function checkVersion(version) {
  if(typeof version!=='string'||!/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/.test(version))throw new Error('invalid portable version');
}
async function hashBuildExecutable(exe) {
  // Cargo's top-level product EXE can be hardlinked to its deps artifact. Read
  // that explicit build input only; never preserve the link in the package.
  // Resources and all output files still require hashFile's single-link rule.
  plainDirectory(dirname(exe));const before=lstatSync(exe);
  if(!before.isFile()||before.isSymbolicLink())throw new Error('build executable must be a plain file');
  const hash=createHash('sha256');let size=0;
  for await(const bytes of createReadStream(exe)){size+=bytes.length;hash.update(bytes);}
  const after=lstatSync(exe);plainDirectory(dirname(exe));
  if(!after.isFile()||after.isSymbolicLink()||size!==before.size||before.size!==after.size||before.mtimeMs!==after.mtimeMs||before.ctimeMs!==after.ctimeMs||before.ino!==after.ino||before.dev!==after.dev||before.nlink!==after.nlink)throw new Error('build executable changed while hashing');
  return {size,sha256:hash.digest('hex')};
}
function peX64(exe) {
  const fd=openSync(exe,'r');
  try {
    const dos=Buffer.alloc(64);if(readSync(fd,dos,0,64,0)!==64||dos.toString('ascii',0,2)!=='MZ')throw new Error('main executable is not PE');
    const offset=dos.readUInt32LE(60);if(offset<64||offset>16*1024*1024)throw new Error('invalid PE header offset');
    const pe=Buffer.alloc(26);if(readSync(fd,pe,0,26,offset)!==26||pe.readUInt32LE(0)!==0x4550||pe.readUInt16LE(4)!==0x8664||pe.readUInt16LE(24)!==0x20b||(pe.readUInt16LE(22)&0x2000)!==0)throw new Error('portable requires a Windows x64 PE executable, not another target or DLL');
  } finally {closeSync(fd);}
}
function walkFiles(root,prefix='') {
  plainDirectory(root);const files=[];
  for(const name of readdirSync(root).sort()) {
    const rel=prefix+name;relativePath(rel);const file=join(root,name),stat=lstatSync(file);
    if(stat.isSymbolicLink())throw new Error('portable input contains a link');
    if(stat.isDirectory())files.push(...walkFiles(file,rel+'/'));
    else if(stat.isFile()&&stat.nlink===1)files.push(rel);
    else throw new Error('portable input contains a hardlink or special file');
    if(files.length>MAX_FILES)throw new Error('too many portable files');
  }
  return files;
}
function resourceFiles(resources,config) {
  // Keep the resource mapping identical to the Windows Tauri bundle. A future
  // layout change must update this mapping and its tests, not silently omit it.
  const patterns=JSON.parse(readFileSync(config,'utf8')).bundle?.resources;
  if(!Array.isArray(patterns)||JSON.stringify([...patterns].sort())!==JSON.stringify([...expectedResources].sort()))throw new Error('Windows resource mapping changed; portable mapping needs review');
  plainDirectory(resources);const files=[];
  for(const pattern of patterns) {
    const recursive=pattern.endsWith('/**/*'),flat=!recursive&&pattern.endsWith('/*');
    const name=pattern.slice('resources/'.length).replace(/\/(?:\*\*\/)?\*$/,'');relativePath(name);
    if(recursive) {
      const children=walkFiles(join(resources,name));if(!children.length)throw new Error('empty required resource directory');
      files.push(...children.map(child=>name+'/'+child));
    } else if(flat) {
      plainDirectory(join(resources,name));const children=readdirSync(join(resources,name)).sort();
      if(!children.length)throw new Error('empty required resource directory');
      for(const child of children){relativePath(child);const stat=lstatSync(join(resources,name,child));if(!stat.isFile()||stat.isSymbolicLink()||stat.nlink!==1)throw new Error('non-plain file in flat resource mapping');files.push(name+'/'+child);}
    } else files.push(name);
  }
  const names=new Set();
  for(const file of files){relativePath(file);if(names.has(file.toUpperCase()))throw new Error('duplicate resource destination');names.add(file.toUpperCase());}
  if(files.length>MAX_FILES)throw new Error('too many resource files');
  const seed=JSON.parse(readFileSync(join(resources,'computer-use/seed/manifest.json'),'utf8'));
  if(seed.schema_version!==2||seed.components?.['js-runtime']?.arch!==TARGET||seed.components?.chromium?.arch!==TARGET)throw new Error('runtime seed target mismatch');
  return files.sort();
}
function readme(version) {
  return `Grok App portable (绿色版) v${version}\n================================\n1. 解压完整目录，双击 Grok.exe；不要单独移动 EXE 或删除 resources。\n2. Computer Use 的 Node、Playwright 与 Chromium 随包提供，不依赖用户安装 Node/Rust。\n3. 仍需系统 Microsoft Edge WebView2 Runtime；Agent 会话需 Grok Build CLI 与登录。\n4. 更新或回退时使用完整版本目录；退出旧版本后保留旧目录可用于切换。\n5. 应用数据仍保存在应用数据目录，本 ZIP 不包含账号、用户浏览器 profile 或凭据。\n\nExtract the complete directory and run Grok.exe. Keep resources beside it.\nComputer Use includes its private Node, Playwright and Chromium runtime; no system Node/Rust is required.\nWebView2 is required. Agent sessions still need Grok Build CLI and login.\nFor portable version replacement/rollback, use the entire version directory, not only the EXE.\nApplication data is stored separately. This archive contains no user accounts or browser profiles.\n`;
}
export async function verifyPortable(root) {
  root=resolve(root);plainDirectory(root);
  const manifestPath=join(root,MANIFEST);const manifestIdentity=await hashFile(manifestPath);
  if(manifestIdentity.size>2*1024*1024)throw new Error('oversized portable manifest');
  const manifest=JSON.parse(readFileSync(manifestPath,'utf8'));
  if(!manifest||Object.keys(manifest).sort().join(',')!=='executable,files,format,schema,target,version'||manifest.format!==FORMAT||manifest.schema!==1||manifest.target!==TARGET||manifest.executable!=='Grok.exe'||!Array.isArray(manifest.files)||!manifest.files.length||manifest.files.length>MAX_FILES)throw new Error('invalid portable manifest');
  checkVersion(manifest.version);const expected=new Set(),aliases=new Set();
  for(const file of manifest.files) {
    if(!file||Object.keys(file).sort().join(',')!=='path,sha256,size')throw new Error('invalid portable file entry');
    relativePath(file.path);
    if(file.path===MANIFEST||aliases.has(file.path.toUpperCase())||!Number.isSafeInteger(file.size)||file.size<0||typeof file.sha256!=='string'||!/^[a-f0-9]{64}$/.test(file.sha256))throw new Error('invalid portable file identity');
    aliases.add(file.path.toUpperCase());expected.add(file.path);
    const actual=await hashFile(join(root,file.path));
    if(actual.size!==file.size||actual.sha256!==file.sha256)throw new Error(`portable content mismatch: ${file.path}`);
  }
  const actualFiles=walkFiles(root).filter(file=>file!==MANIFEST);
  if(actualFiles.length!==expected.size||actualFiles.some(file=>!expected.has(file)))throw new Error('portable file set differs from manifest');
  for(const required of ['Grok.exe','README-portable.txt','resources/computer-use/windows-x64.lock.json','resources/computer-use/pack-targets.json','resources/computer-use/README.md','resources/computer-use/seed/manifest.json','resources/computer-use/seed/bin/node.exe','resources/computer-use/seed/playwright/worker.mjs','resources/computer-use/seed/chromium/chrome-win/chrome.exe'])if(!expected.has(required))throw new Error(`portable required file missing: ${required}`);
  const seed=JSON.parse(readFileSync(join(root,'resources/computer-use/seed/manifest.json'),'utf8'));
  if(seed.schema_version!==2||seed.components?.['js-runtime']?.arch!==TARGET||seed.components?.chromium?.arch!==TARGET)throw new Error('runtime seed target mismatch');
  peX64(join(root,'Grok.exe'));
  if(JSON.stringify(manifestIdentity)!==JSON.stringify(await hashFile(manifestPath)))throw new Error('portable manifest changed during verification');
  return {version:manifest.version,target:TARGET,files:manifest.files.length,bytes:manifest.files.reduce((n,f)=>n+f.size,0),manifestSha256:manifestIdentity.sha256,signed:false};
}
export async function stagePortable({exe,resources,config,stage,version}) {
  checkVersion(version);exe=resolve(exe);resources=resolve(resources);stage=resolve(stage);config=resolve(config);
  if(!['grok-app.exe','Grok.exe'].includes(basename(exe)))throw new Error('explicit product executable is required; arbitrary EXE fallback is forbidden');
  if(inside(resources,stage)||inside(stage,resources)||inside(stage,exe))throw new Error('portable output overlaps source');
  if(entryExists(stage))throw new Error('portable destination already exists');
  const originalExe=await hashBuildExecutable(exe);peX64(exe);
  const files=resourceFiles(resources,config);
  makePlainDirectory(dirname(stage));
  const temporary=stage+'.staging-'+randomUUID();mkdirSync(temporary);
  try {
    const entries=[];
    async function copy(source,destination,sourceHash=hashFile) {
      const before=await sourceHash(source);const to=join(temporary,destination);mkdirSync(dirname(to),{recursive:true});
      copyFileSync(source,to,constants.COPYFILE_EXCL);const copied=await hashFile(to),after=await sourceHash(source);
      if(JSON.stringify(before)!==JSON.stringify(copied)||JSON.stringify(before)!==JSON.stringify(after))throw new Error('source changed while copying portable files');
      entries.push({path:destination,...copied});
    }
    await copy(exe,'Grok.exe',hashBuildExecutable);
    for(const file of files)await copy(join(resources,file),'resources/'+file);
    if(JSON.stringify(originalExe)!==JSON.stringify(await hashBuildExecutable(exe)))throw new Error('main executable changed during packaging');
    writeFileSync(join(temporary,'README-portable.txt'),readme(version),{flag:'wx'});
    entries.push({path:'README-portable.txt',...await hashFile(join(temporary,'README-portable.txt'))});
    entries.sort((a,b)=>Buffer.compare(Buffer.from(a.path),Buffer.from(b.path)));
    writeFileSync(join(temporary,MANIFEST),JSON.stringify({schema:1,format:FORMAT,version,target:TARGET,executable:'Grok.exe',files:entries},null,2)+'\n',{flag:'wx'});
    const report=await verifyPortable(temporary);
    if(entryExists(stage))throw new Error('portable destination appeared during build');
    renameSync(temporary,stage);return {...report,stage};
  } catch(error) {rmSync(temporary,{recursive:true,force:true});throw error;}
}
if(process.argv[1]&&import.meta.url===pathToFileURL(resolve(process.argv[1])).href) {
  try {
    const args=process.argv.slice(2);
    if(args.length===2&&args[0]==='--verify')console.log(JSON.stringify(await verifyPortable(args[1])));
    else {
      const allowed=new Set(['exe','resources','config','stage','version']),options={};
      for(let i=0;i<args.length;i+=2){const key=args[i].slice(2);if(!args[i].startsWith('--')||!allowed.has(key)||options[key]||!args[i+1]||args[i+1].startsWith('--'))throw new Error('invalid staging argument');options[key]=args[i+1];}
      if([...allowed].some(key=>!options[key]))throw new Error('exe, resources, config, stage and version are required');
      console.log(JSON.stringify(await stagePortable(options)));
    }
  } catch(error){console.error(`Portable package: ${error.message}`);process.exitCode=1;}
}
