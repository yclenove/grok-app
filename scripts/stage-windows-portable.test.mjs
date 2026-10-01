// Fixture-only copying/integrity tests. All fixture EXEs are inert and never run.
import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, readdirSync, rmSync, existsSync, linkSync, symlinkSync, copyFileSync, lstatSync, renameSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { stagePortable, verifyPortable } from './stage-windows-portable.mjs';
import { hashFile } from './windows-install-inventory.mjs';

const here=dirname(fileURLToPath(import.meta.url));
function fixture(t) {
  const root=mkdtempSync(join(tmpdir(),'grok-portable-test-'));
  t.after(()=>rmSync(root,{recursive:true,force:true}));
  const resources=join(root,'source/resources'),exe=join(root,'source/grok-app.exe');
  const config=join(root,'windows.json'),stage=join(root,'output/Grok_0.2.33_x64-portable');
  function put(path,value) {mkdirSync(dirname(path),{recursive:true});writeFileSync(path,value);}
  const pe=Buffer.alloc(128);pe.write('MZ');pe.writeUInt32LE(64,60);pe.writeUInt32LE(0x4550,64);pe.writeUInt16LE(0x8664,68);pe.writeUInt16LE(0x20b,88);
  put(exe,pe);
  put(config,readFileSync(join(here,'../src-tauri/tauri.windows.conf.json')));
  const files={
    'computer-use/windows-x64.lock.json':'{}',
    'computer-use/pack-targets.json':'{}',
    'computer-use/README.md':'fixture source notice',
    'computer-use/seed/manifest.json':JSON.stringify({schema_version:2,components:{'js-runtime':{arch:'x86_64-windows'},chromium:{arch:'x86_64-windows'}}}),
    'computer-use/seed/bin/node.exe':'inert fixture node, not executed',
    'computer-use/seed/playwright/worker.mjs':'// inert fixture worker',
    'computer-use/seed/playwright/node_modules/playwright-core/index.js':'// inert fixture module',
    'computer-use/seed/playwright/模块/证据.txt':'中文 resource bytes',
    'computer-use/seed/chromium/chrome-win/chrome.exe':'inert fixture browser, not executed',
    'computer-use/seed/chromium/chrome-win/locales/zh-CN.pak':'fixture locale',
  };
  for(const [name,value] of Object.entries(files))put(join(resources,name),value);
  return {root,resources,exe,config,stage,version:'0.2.33',put,files};
}
async function rejectsNoStage(f,pattern) {
  await assert.rejects(stagePortable(f),pattern);
  assert.equal(existsSync(f.stage),false);
  if(existsSync(dirname(f.stage)))assert.equal(readdirSync(dirname(f.stage)).some(n=>n.includes('.staging-')),false);
}

test('copies the complete Windows resource mapping and verifies exact bytes',async t=>{
  const f=fixture(t),original=await hashFile(f.exe);
  const result=await stagePortable(f);
  assert.equal(result.files,Object.keys(f.files).length+2);
  assert.equal(result.signed,false);
  assert.equal(result.version,f.version);
  for(const [name,value] of Object.entries(f.files))assert.equal(readFileSync(join(f.stage,'resources',name),'utf8'),value);
  assert.deepEqual(await hashFile(join(f.stage,'Grok.exe')),original);
  assert.deepEqual(await hashFile(f.exe),original);
  assert.match(readFileSync(join(f.stage,'README-portable.txt'),'utf8'),/Keep resources beside it/);
  assert.equal((await verifyPortable(f.stage)).manifestSha256,result.manifestSha256);
});
test('ships only declared Windows resources, not platform siblings or caches',async t=>{
  const f=fixture(t);f.put(join(f.resources,'computer-use/linux-x64.lock.json'),'not included');
  f.put(join(f.resources,'computer-use/.cache/secret.txt'),'not included');
  f.put(join(f.resources,'computer-use/.cu-mutation.lock'),'');
  f.put(join(f.resources,'computer-use/.cu-mutation.lock.owner.json'),'{"repo":"private build directory"}');
  await stagePortable(f);
  assert.equal(existsSync(join(f.stage,'resources/computer-use/linux-x64.lock.json')),false);
  assert.equal(existsSync(join(f.stage,'resources/computer-use/.cache')),false);
  assert.equal(existsSync(join(f.stage,'resources/computer-use/.cu-mutation.lock')),false);
  assert.equal(existsSync(join(f.stage,'resources/computer-use/.cu-mutation.lock.owner.json')),false);
});
test('Cargo hardlinked EXE input becomes an independent verified package file',async t=>{
  const f=fixture(t),alias=join(f.root,'cargo-deps.exe');linkSync(f.exe,alias);
  assert.equal(lstatSync(f.exe).nlink,2);await stagePortable(f);
  const packaged=join(f.stage,'Grok.exe'),identity=await hashFile(packaged);
  assert.equal(lstatSync(packaged).nlink,1);
  f.put(alias,'simulate a later build replacing artifact bytes');
  assert.deepEqual(await hashFile(packaged),identity);
  await verifyPortable(f.stage);
});
test('an existing destination and its contents are never deleted',async t=>{
  const f=fixture(t);f.put(join(f.stage,'sentinel'),'keep');
  await assert.rejects(stagePortable(f),/already exists/);
  assert.equal(readFileSync(join(f.stage,'sentinel'),'utf8'),'keep');
});
for(const [name,mutate,pattern] of [
  ['missing browser tree',f=>rmSync(join(f.resources,'computer-use/seed/chromium'),{recursive:true}),/ENOENT/],
  ['empty browser tree',f=>{rmSync(join(f.resources,'computer-use/seed/chromium/chrome-win'),{recursive:true});mkdirSync(join(f.resources,'computer-use/seed/chromium/chrome-win'));},/empty required/],
  ['missing required worker',f=>rmSync(join(f.resources,'computer-use/seed/playwright/worker.mjs')),/required file missing/],
  ['changed Tauri resource mapping',f=>{const c=JSON.parse(readFileSync(f.config));c.bundle.resources.pop();f.put(f.config,JSON.stringify(c));},/mapping changed/],
  ['wrong seed target',f=>{const p=join(f.resources,'computer-use/seed/manifest.json'),s=JSON.parse(readFileSync(p));s.components.chromium.arch='aarch64-macos';f.put(p,JSON.stringify(s));},/target mismatch/],
  ['arbitrary executable fallback',f=>{const p=join(dirname(f.exe),'cu-probe.exe');copyFileSync(f.exe,p);f.exe=p;},/explicit product/],
  ['x86 executable',f=>{const b=readFileSync(f.exe);b.writeUInt16LE(0x14c,68);f.put(f.exe,b);},/Windows x64 PE/],
  ['DLL posing as executable',f=>{const b=readFileSync(f.exe);b.writeUInt16LE(0x2000,86);f.put(f.exe,b);},/Windows x64 PE/],
  ['non-PE executable',f=>f.put(f.exe,'not PE'),/not PE/],
  ['version path traversal',f=>{f.version='../escape';},/invalid portable version/],
  ['hardlinked source',f=>{const p=join(f.resources,'computer-use/seed/bin/node.exe');linkSync(p,join(f.root,'node-hardlink'));},/non-plain|hardlink/],
  ['stage inside resource source',f=>{f.stage=join(f.resources,'new-stage');},/overlaps source/],
])test(`refuses ${name} without publishing a partial stage`,async t=>{const f=fixture(t);mutate(f);await rejectsNoStage(f,pattern);});

test('refuses a junction before creating descendants outside its output root',async t=>{
  const f=fixture(t),other=join(f.root,'other'),junction=join(f.root,'junction');mkdirSync(other);
  symlinkSync(other,junction,process.platform==='win32'?'junction':'dir');
  f.stage=join(junction,'must-not-create','stage');
  await assert.rejects(stagePortable(f),/link or non-directory/);
  assert.equal(existsSync(join(other,'must-not-create')),false);
});
test('rejects junctions embedded in declared recursive resource trees',async t=>{
  const f=fixture(t),other=join(f.root,'other');mkdirSync(other);
  symlinkSync(other,join(f.resources,'computer-use/seed/playwright/linked'),process.platform==='win32'?'junction':'dir');
  await rejectsNoStage(f,/contains a link/);
});
for(const [name,mutate,pattern] of [
  ['modified content',f=>f.put(join(f.stage,'resources/computer-use/seed/bin/node.exe'),'modified'),/content mismatch/],
  ['extra unlisted file',f=>f.put(join(f.stage,'extra.exe'),'unlisted'),/file set differs/],
  ['missing listed file',f=>rmSync(join(f.stage,'Grok.exe')),/ENOENT/],
  ['duplicate case-alias entry',(f,m)=>m.files.push({...m.files[0],path:m.files[0].path.toUpperCase()}),/invalid portable file identity/],
  ['unsafe manifest path',(f,m)=>{m.files[0].path='../escape';},/unsafe Windows/],
  ['unknown envelope field',(f,m)=>{m.trusted=true;},/invalid portable manifest/],
  ['unknown file field',(f,m)=>{m.files[0].ignore=true;},/invalid portable file entry/],
])test(`verification rejects ${name}`,async t=>{
  const f=fixture(t);await stagePortable(f);
  const file=join(f.stage,'portable-files.json'),manifest=JSON.parse(readFileSync(file));
  mutate(f,manifest);f.put(file,JSON.stringify(manifest));
  await assert.rejects(verifyPortable(f.stage),pattern);
});
test('removing a required file from both disk and integrity manifest still fails',async t=>{
  const f=fixture(t);await stagePortable(f);
  const file=join(f.stage,'portable-files.json'),manifest=JSON.parse(readFileSync(file));
  const name='resources/computer-use/seed/bin/node.exe';
  rmSync(join(f.stage,name));manifest.files=manifest.files.filter(e=>e.path!==name);f.put(file,JSON.stringify(manifest));
  await assert.rejects(verifyPortable(f.stage),/required file missing/);
});
test('CLI refuses incomplete options rather than guessing a product executable',t=>{
  const f=fixture(t);
  const p=spawnSync(process.execPath,[resolve(here,'stage-windows-portable.mjs'),'--stage',f.stage],{encoding:'utf8'});
  assert.equal(p.status,1);assert.match(p.stderr,/are required/);assert.equal(existsSync(f.stage),false);
});
test('Windows ZIP roundtrip preserves Unicode, refuses overwrite, and rejects corrupted staging',{skip:process.platform!=='win32'},async t=>{
  const f=fixture(t);await stagePortable(f);
  const archiveParent=join(f.root,'long-build-output-'+'nested-'.repeat(11));mkdirSync(archiveParent);
  const archive=join(archiveParent,'portable.zip');
  const pack=target=>spawnSync('powershell.exe',['-NoProfile','-NonInteractive','-File',join(here,'package-windows-portable.ps1'),'-Stage',f.stage,'-Archive',target],{encoding:'utf8',timeout:60000});
  const inside=pack(join(f.stage,'inside.zip'));
  assert.notEqual(inside.status,0);assert.match(inside.stderr,/outside the staged image/);assert.equal(existsSync(join(f.stage,'inside.zip')),false);
  const good=pack(archive);assert.equal(good.status,0,good.stdout+'\n'+good.stderr);
  const identity=await hashFile(archive),overwrite=pack(archive);
  assert.notEqual(overwrite.status,0);assert.match(overwrite.stderr,/already exists/);
  assert.deepEqual(await hashFile(archive),identity);
  f.put(join(f.stage,'resources/computer-use/seed/bin/node.exe'),'corrupt');
  const bad=pack(join(archiveParent,'corrupt.zip'));
  assert.notEqual(bad.status,0);assert.match(bad.stderr,/content mismatch|source verification failed/);
  assert.equal(existsSync(join(archiveParent,'corrupt.zip')),false);
  assert.equal(readdirSync(archiveParent).some(n=>n.includes('.partial-')||n.includes('.verify-')),false);
});
test('Windows archive treats switch-like stage names as paths and never deletes the input',{skip:process.platform!=='win32'},async t=>{
  const f=fixture(t);await stagePortable(f);
  const renamed=join(dirname(f.stage),'-sdel');renameSync(f.stage,renamed);f.stage=renamed;
  const sourceIdentity=await hashFile(f.exe);
  const p=spawnSync('powershell.exe',['-NoProfile','-NonInteractive','-File',join(here,'package-windows-portable.ps1'),'-Stage',f.stage,'-Archive',join(f.root,'switch-safe.zip')],{encoding:'utf8',timeout:60000});
  assert.equal(p.status,0,p.stdout+'\n'+p.stderr);
  assert.deepEqual(await hashFile(f.exe),sourceIdentity);
  await verifyPortable(f.stage);
});
