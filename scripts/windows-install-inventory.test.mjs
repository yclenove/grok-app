import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync, linkSync, symlinkSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { createInventory, relativePath, validateInventory } from './windows-install-inventory.mjs';
import { attachInventories } from './attach-windows-install-inventory.mjs';

function fixture(t) {
  const base=mkdtempSync(join(tmpdir(),'grok-inventory-owned-')); t.after(()=>rmSync(base,{recursive:true,force:true}));
  const root=join(base,'image');mkdirSync(root);mkdirSync(join(root,'resources'));
  writeFileSync(join(root,'grok-app.exe'),'owned main');writeFileSync(join(root,'resources','中文.txt'),'owned data');
  const payload=join(base,'Grok_1.2.3_x64-setup.exe');writeFileSync(payload,'MZ owned not an installer');
  return {base,root,payload,appName:'Grok',version:'1.2.3',target:'windows',arch:'x86_64',kind:'Nsis',executable:'grok-app.exe'};
}

test('inventory hashes exact extracted image bytes deterministically without executing payload',async t=>{
  const f=fixture(t),a=await createInventory(f),b=await createInventory(f);
  assert.deepEqual(a,b);assert.equal(a.files.length,2);
  assert.equal(a.payload_sha256,createHash('sha256').update(readFileSync(f.payload)).digest('hex'));
  assert.deepEqual(a.files.find(f=>f.path==='resources/中文.txt'),{path:'resources/中文.txt',size:10,sha256:createHash('sha256').update('owned data').digest('hex')});
});
test('NSIS completion contract is explicit, publisher-bound and absent for legacy images',async t=>{
  const f=fixture(t),legacy=await createInventory(f);
  assert.equal(legacy.completion,undefined);
  const contract={protocol:'grok-nsis-install-complete-v1',bundle_id:'com.grokapp.desktop',install_scope:'currentUser'};
  const value=await createInventory({...f,nsisImage:true,completion:contract});
  assert.deepEqual(value.completion,contract);
  assert.equal(value.payload_sha256,legacy.payload_sha256);
  for(const completion of [null,{...contract,protocol:'unknown'},{...contract,install_scope:'allUsers'},{...contract,bundle_id:'bad\nidentity'},{...contract,extra:true}]) {
    assert.throws(()=>validateInventory({...value,completion}));
  }
  await assert.rejects(createInventory({...f,completion:contract}));
  await assert.rejects(createInventory({...f,kind:'Msi',completion:contract}));
});
test('NSIS bootstrap exclusion is explicit and does not exclude application subdirectories',async t=>{
  const f=fixture(t);mkdirSync(join(f.root,'$PLUGINSDIR'));writeFileSync(join(f.root,'$PLUGINSDIR','bootstrap.dll'),'plugin');
  writeFileSync(join(f.root,'uninstall.exe'),'generated only');writeFileSync(join(f.root,'resources','uninstall.exe'),'application data');
  assert.equal((await createInventory(f)).files.length,5);
  const scoped=await createInventory({...f,nsisImage:true});assert.equal(scoped.files.length,3);
  assert(scoped.files.some(x=>x.path==='resources/uninstall.exe'));
  await assert.rejects(createInventory({...f,nsisImage:true,kind:'Msi'}));
});
test('failure protocol requires an explicit supported signed-contract field',async t=>{
  const f=fixture(t),completion={protocol:'grok-nsis-install-complete-v1',bundle_id:'com.grokapp.desktop',install_scope:'currentUser',failure_protocol:'grok-nsis-install-failed-v1'};
  const value=await createInventory({...f,nsisImage:true,completion});
  assert.deepEqual(value.completion,completion);
  for(const bad of [null,false,[],{},'','unknown','grok-nsis-install-complete-v1']) {
    assert.throws(()=>validateInventory({...value,completion:{...completion,failure_protocol:bad}}));
  }
});
test('CLI failure receipt opt-in is explicit, bound to completion, and attached without rewriting',async t=>{
  const f=fixture(t),output=f.payload+'.install-inventory.json';
  const args=[resolve('scripts/windows-install-inventory.mjs'),'--root',f.root,'--payload',f.payload,'--app-name',f.appName,'--version',f.version,'--target',f.target,'--arch',f.arch,'--kind',f.kind,'--executable',f.executable,'--output',output,'--nsis-image'];
  for(const extras of [['--nsis-failure-receipt'],['--nsis-completion-bundle-id','com.grokapp.desktop','--nsis-failure-receipt','--nsis-failure-receipt']]) {
    const rejected=spawnSync(process.execPath,[...args,...extras],{encoding:'utf8'});
    assert.notEqual(rejected.status,0);assert.equal(existsSync(output),false);
  }
  const result=spawnSync(process.execPath,[...args,'--nsis-completion-bundle-id','com.grokapp.desktop','--nsis-failure-receipt'],{encoding:'utf8'});
  assert.equal(result.status,0,result.stderr);
  const document=readFileSync(output,'utf8');
  assert.equal(JSON.parse(document).completion.failure_protocol,'grok-nsis-install-failed-v1');
  writeFileSync(output+'.sig','TEST-ONLY-NOT-A-CRYPTO-ACCEPTANCE');
  const latest={version:f.version,platforms:{'windows-x86_64-nsis':{url:'https://example.invalid/Grok_1.2.3_x64-setup.exe',signature:'payload-signature'}}};
  assert.equal((await attachInventories(latest,f.base)).platforms['windows-x86_64-nsis'].windows_install_inventory.document,document);
});
test('CLI completion declaration survives exact metadata attachment and rejects invalid scope',async t=>{
  const f=fixture(t),output=f.payload+'.install-inventory.json';
  const args=[resolve('scripts/windows-install-inventory.mjs'),'--root',f.root,'--payload',f.payload,'--app-name',f.appName,'--version',f.version,'--target',f.target,'--arch',f.arch,'--kind',f.kind,'--executable',f.executable,'--output',output,'--nsis-image','--nsis-completion-bundle-id','com.grokapp.desktop'];
  const produced=spawnSync(process.execPath,args,{encoding:'utf8'});assert.equal(produced.status,0,produced.stderr);
  const document=readFileSync(output,'utf8'),contract=JSON.parse(document).completion;
  assert.deepEqual(contract,{protocol:'grok-nsis-install-complete-v1',bundle_id:'com.grokapp.desktop',install_scope:'currentUser'});
  writeFileSync(output+'.sig','TEST-ONLY-METADATA-NOT-A-CRYPTO-ACCEPTANCE');
  const latest={version:f.version,platforms:{'windows-x86_64-nsis':{url:'https://example.invalid/Grok_1.2.3_x64-setup.exe',signature:'payload-signature'}}};
  const attached=await attachInventories(latest,f.base);
  assert.equal(attached.platforms['windows-x86_64-nsis'].windows_install_inventory.document,document);
  const changed=JSON.parse(document);changed.completion.install_scope='allUsers';writeFileSync(output,JSON.stringify(changed));
  await assert.rejects(attachInventories(latest,f.base));
});
test('Windows unsafe paths, devices and aliases are rejected',()=>{
  for(const path of ['', '../a','/a','a\\b','C:/a','a:b','a//b','a/./b','file.','file ','CON','NUL.txt','COM¹.dat','LPT9','CLOCK$','a~1','a?b','a\0b']) assert.throws(()=>relativePath(path),path);
  relativePath('资源/中文🙂.txt');
});
test('inventory rejects case collisions, file-as-directory and missing executable',async t=>{
  const f=fixture(t),value=await createInventory(f);
  for(const path of ['GROK-APP.EXE','resources']) {
    const changed=structuredClone(value);changed.files.push({...value.files[0],path});assert.throws(()=>validateInventory(changed));
  }
  const changed=structuredClone(value);changed.files=changed.files.filter(x=>x.path!==changed.executable);assert.throws(()=>validateInventory(changed));
});
test('producer rejects a native hardlink instead of trusting path spelling',async t=>{
  const f=fixture(t);linkSync(join(f.root,'grok-app.exe'),join(f.base,'owned-hardlink'));
  await assert.rejects(createInventory(f),/hardlinked/);
});
test('producer rejects native junctions and does not follow them outside its image',async t=>{
  const f=fixture(t),other=join(f.base,'outside');mkdirSync(other);writeFileSync(join(other,'unread.txt'),'owned outside');
  symlinkSync(other,join(f.root,'linked'),process.platform==='win32'?'junction':'dir');
  await assert.rejects(createInventory(f),/reparse link/);
  assert.equal(readFileSync(join(other,'unread.txt'),'utf8'),'owned outside');
});
test('signed metadata attachment preserves exact document bytes and selected candidate',async t=>{
  const f=fixture(t),document=JSON.stringify(await createInventory(f),null,2)+'\n';
  writeFileSync(f.payload+'.install-inventory.json',document);writeFileSync(f.payload+'.install-inventory.json.sig','TEST-ONLY-ATTACHMENT-NOT-A-CRYPTO-ACCEPTANCE\n');
  const latest={version:f.version,platforms:{'windows-x86_64':{url:'https://example.invalid/Grok_1.2.3_x64-setup.exe',signature:'payload-signature'},'darwin-aarch64':{url:'https://example.invalid/mac.tar.gz',signature:'untouched'}}};
  const result=await attachInventories(latest,f.base);
  assert.equal(result.platforms['windows-x86_64'].windows_install_inventory.document,document);
  assert.equal(result.platforms['windows-x86_64'].windows_install_inventory.signature,'TEST-ONLY-ATTACHMENT-NOT-A-CRYPTO-ACCEPTANCE');
  assert.equal(result.platforms['darwin-aarch64'].windows_install_inventory,undefined);
});
test('attachment refuses missing sidecar and byte-swapped or wrong-version payload',async t=>{
  const f=fixture(t),make=()=>({version:f.version,platforms:{'windows-x86_64':{url:'https://example.invalid/Grok_1.2.3_x64-setup.exe',signature:'p'}}});
  await assert.rejects(attachInventories(make(),f.base));
  const value=await createInventory(f);writeFileSync(f.payload+'.install-inventory.json',JSON.stringify({...value,version:'9.9.9'}));writeFileSync(f.payload+'.install-inventory.json.sig','test-only');
  await assert.rejects(attachInventories(make(),f.base),/does not match/);
  writeFileSync(f.payload+'.install-inventory.json',JSON.stringify(value));writeFileSync(f.payload,'MZ replacement');
  await assert.rejects(attachInventories(make(),f.base),/does not match/);
});
test('CLI writes a new unsigned document once and never overwrites an existing receipt',t=>{
  const f=fixture(t),output=join(f.base,'inventory.json');
  const args=[resolve('scripts/windows-install-inventory.mjs'),'--root',f.root,'--payload',f.payload,'--app-name',f.appName,'--version',f.version,'--target',f.target,'--arch',f.arch,'--kind',f.kind,'--executable',f.executable,'--output',output];
  const first=spawnSync(process.execPath,args,{encoding:'utf8'});assert.equal(first.status,0,first.stderr);
  const bytes=readFileSync(output);assert.equal(JSON.parse(first.stdout).signed,false);
  const second=spawnSync(process.execPath,args,{encoding:'utf8'});assert.notEqual(second.status,0);assert.deepEqual(readFileSync(output),bytes);
});

test('hash fields must be JSON strings rather than coercible arrays',async t=>{
  const value=await createInventory(fixture(t));
  const envelope=structuredClone(value);envelope.payload_sha256=[value.payload_sha256];
  assert.throws(()=>validateInventory(envelope));
  const entry=structuredClone(value);entry.files[0].sha256=[value.files[0].sha256];
  assert.throws(()=>validateInventory(entry));
});

test('producer refuses payload or output inside the installed image',async t=>{
  const f=fixture(t);
  await assert.rejects(createInventory({...f,payload:join(f.root,'grok-app.exe')}),/outside/);
  const output=join(f.root,'resources','inventory.json');
  const args=[resolve('scripts/windows-install-inventory.mjs'),'--root',f.root,'--payload',f.payload,'--app-name',f.appName,'--version',f.version,'--target',f.target,'--arch',f.arch,'--kind',f.kind,'--executable',f.executable,'--output',output];
  const result=spawnSync(process.execPath,args,{encoding:'utf8'});
  assert.notEqual(result.status,0);assert.match(result.stderr,/outside/);assert.equal(existsSync(output),false);
});

test('attachment requires exact platform aliases and installer kind',async t=>{
  const f=fixture(t),value=await createInventory(f);
  writeFileSync(f.payload+'.install-inventory.json',JSON.stringify(value));
  writeFileSync(f.payload+'.install-inventory.json.sig','TEST-ONLY-NOT-A-SIGNATURE');
  const make=key=>({version:f.version,platforms:{[key]:{url:'https://example.invalid/Grok_1.2.3_x64-setup.exe',signature:'p'}}});
  for(const key of ['windows-x86_64oops','windows-x86_64-nsis-extra','windows-x86_64-msi']) {
    await assert.rejects(attachInventories(make(key),f.base),/does not match/);
  }
  await attachInventories(make('windows-x86_64-nsis'),f.base);
  writeFileSync(f.payload+'.install-inventory.json',JSON.stringify({...value,kind:'Msi'}));
  await assert.rejects(attachInventories(make('windows-x86_64'),f.base),/does not match/);
});

test('attachment preserves NSIS and MSI archive metadata compatibility without treating a ZIP as the other installer kind',async t=>{
  const f=fixture(t),value=await createInventory(f);
  for(const [name,kind] of [['Grok_x64.nsis.zip','Nsis'],['Grok_x64.msi.zip','Msi']]) {
    // Metadata-only inert bytes: native Rust archive tests validate actual ZIPs.
    const payload=join(f.base,name);writeFileSync(payload,readFileSync(f.payload));
    writeFileSync(payload+'.install-inventory.json',JSON.stringify({...value,kind}));
    writeFileSync(payload+'.install-inventory.json.sig','TEST-ONLY-NOT-A-SIGNATURE');
    const make=()=>({version:f.version,platforms:{'windows-x86_64':{url:'https://example.invalid/'+name,signature:'p'}}});
    await attachInventories(make(),f.base);
    writeFileSync(payload+'.install-inventory.json',JSON.stringify({...value,kind:kind==='Nsis'?'Msi':'Nsis'}));
    await assert.rejects(attachInventories(make(),f.base),/does not match/);
  }
});
