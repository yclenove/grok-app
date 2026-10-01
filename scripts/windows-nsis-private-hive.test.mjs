import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash, randomUUID} from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import test from 'node:test';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const fixtures = path.join(repo,'scripts/fixtures');
const hook = path.join(repo,'src-tauri/nsis/update-completion.nsh');
const hash = p => createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const escape = s => s.replaceAll('$',()=>'$$').replaceAll('"','$\\"');

test('complete production callback runs behind a process-private hive fence', async t => {
  const nsis=[process.env.GROK_MAKENSIS,process.env.LOCALAPPDATA && path.join(process.env.LOCALAPPDATA,'tauri/NSIS/makensis.exe'),'C:/Program Files (x86)/NSIS/makensis.exe','C:/Program Files/NSIS/makensis.exe'].find(p=>p&&fs.existsSync(p));
  if(process.platform!=='win32'||!nsis) {
    assert.notEqual(process.env.GROK_NSIS_REQUIRED,'1','required Windows NSIS unavailable');
    return t.skip('Windows + NSIS required; mandatory in Windows CI');
  }
  const parent=process.env.GROK_NSIS_TEST_ARTIFACT_DIR;
  const dir=fs.mkdtempSync(path.join(parent||os.tmpdir(),'grok-private-callback-'));
  if(!parent) t.after(()=>fs.rmSync(dir,{recursive:true,force:true}));
  const owned=path.join(dir,'private 空格 $ hive');
  fs.mkdirSync(owned);
  const nonce='a'+randomUUID().slice(1);
  const product=`Owned 回执 ${nonce}`;
  const key=`Software\\GrokCompletionFixture\\${nonce}`;
  const fence=`OwnedFence-${nonce}`;
  const define={COMPLETION_HOOK:hook,FIXTURE_ROOT:owned,PRODUCTNAME:product,UNINSTKEY:`${key}\\Uninstall`,MANUPRODUCTKEY:`${key}\\Product`,FENCE:fence};
  fs.writeFileSync(path.join(dir,'fixture-defines.nsh'),'\uFEFF'+Object.entries(define).map(([k,v])=>`!define ${k} "${escape(v)}"`).join('\n')+'\n');
  fs.writeFileSync(path.join(dir,'inert.bin'),'INERT owned fixture, never an executable');
  const source=fs.readFileSync(path.join(fixtures,'nsis-completion-private-hive.nsi'),'utf8');
  assert.doesNotMatch(source,/\b(?:Exec\w*|DeleteReg\w*|Delete|RMDir|WriteUninstaller)\b/);
  assert.match(source,/RegOverridePredefKey/);
  assert.match(source,/RegOpenKeyExW/);
  assert.match(source,/RegOpenKeyExW\(p 0x80000001, w "\$\{FENCE\}", i 0, i 0x20119/);
  assert.match(source,/SetRegView 64/);
  assert(source.indexOf('RegOpenKeyExW')<source.indexOf('SetRegView 64'));
  assert.match(source,/Function \.onInstSuccess\s+Call GrokWriteUpdateCompletion/);
  assert(source.indexOf('RegOverridePredefKey')<source.indexOf('WriteRegStr SHCTX'));
  fs.writeFileSync(path.join(dir,'fixture.nsi'),'\uFEFF'+source);
  const inspect=(hive)=>JSON.parse(execFileSync('powershell.exe',['-NoProfile','-NonInteractive','-File',path.join(fixtures,'inspect-private-nsis-hive.ps1'),'-Key',key,'-Fence',fence,...(hive?['-Hive',hive]:[])],{encoding:'utf8',timeout:30000,maxBuffer:1024*1024}).replace(/^\uFEFF/,''));
  assert.equal(inspect().hostFixtureKeysAbsent,true);
  let compiled;
  try { compiled=execFileSync(nsis,['/V4','fixture.nsi'],{cwd:dir,encoding:'utf8',timeout:60000,maxBuffer:4*1024*1024}); }
  catch(error) {fs.writeFileSync(path.join(dir,'compile.log'),String(error.stdout||'')+String(error.stderr||''));throw error;}
  fs.writeFileSync(path.join(dir,'compile.log'),compiled);
  assert.doesNotMatch(compiled,/warning \d+/i);
  const cases=['ok','bad-product','bad-version','bad-publisher','bad-main','bad-location','bad-uninstall','bad-root','no-main','no-uninstaller','not-update','bad-nonce','uppercase-nonce','bad-variant','short-nonce','nonhex-nonce','missing-name','missing-receipt','existing','hive-failure','override-failure'];
  const observations=[];
  for(const name of cases) {
    await t.test(name,()=>{
      const receipt=path.join(owned,`${name}.receipt`);
      const sentinel=Buffer.from(`original ${nonce}`);
      if(name==='existing') fs.writeFileSync(receipt,sentinel);
      let argumentNonce=nonce;
      if(name==='bad-nonce') argumentNonce=nonce.slice(0,14)+'1'+nonce.slice(15);
      if(name==='uppercase-nonce') argumentNonce=nonce.toUpperCase();
      if(name==='bad-variant') argumentNonce=nonce.slice(0,19)+'0'+nonce.slice(20);
      if(name==='short-nonce') argumentNonce=nonce.slice(1);
      if(name==='nonhex-nonce') argumentNonce='g'+nonce.slice(1);
      const native=JSON.parse(execFileSync('powershell.exe',['-NoProfile','-NonInteractive','-File',path.join(fixtures,'run-private-nsis.ps1'),'-Binary',path.join(dir,'owned-private-callback.exe'),'-Owned',owned,'-Case',name,'-Nonce',argumentNonce],{encoding:'utf8',timeout:30000,maxBuffer:1024*1024}).replace(/^\uFEFF/,''));
      fs.writeFileSync(path.join(dir,`${name}-process.json`),JSON.stringify(native,null,2)+'\n');
      assert.equal(native.handleHeld,true);
      assert(BigInt(native.exited)>BigInt(native.created));
      const isolationFailure=name.endsWith('-failure');
      const isolationError=path.join(owned,`${name}.isolation-error`);
      assert.equal(native.exitCode,isolationFailure?73:0,fs.existsSync(isolationError)?fs.readFileSync(isolationError).toString('utf16le'):name);
      if(isolationFailure) {
        const error=fs.readFileSync(isolationError).toString('utf16le').replace(/^\uFEFF/,'').split('\r\n');
        assert.equal(error[0],name==='hive-failure'?'load-hive':'override');
        assert.notEqual(error[1],'0');
      }
      assert.equal(fs.existsSync(path.join(owned,`${name}.callback-ran`)),!isolationFailure);
      let registration;
      if(!isolationFailure) {
        registration=inspect(path.join(owned,`${name}.hiv`));
        assert.equal(registration.privateRegistryView,64);
        const install=path.join(owned,name);
        const expected={DisplayName:product,DisplayVersion:'1.2.3',Publisher:'Owned callback fixture',MainBinaryName:'owned-main.exe',InstallLocation:`"${install}"`,UninstallString:`"${install}\\uninstall.exe"`};
        const bad={'bad-product':['DisplayName','different'],'bad-version':['DisplayVersion','0.0.0'],'bad-publisher':['Publisher','different'],'bad-main':['MainBinaryName','different.exe'],'bad-location':['InstallLocation',install],'bad-uninstall':['UninstallString','different']};
        if(bad[name]) expected[bad[name][0]]=bad[name][1];
        if(name==='missing-name') expected.DisplayName=null;
        assert.deepEqual(registration.privateValues,expected);
        assert.equal(registration.privateInstallRoot,name==='bad-root'?'different':install);
      } else {
        registration=inspect();
        assert.equal(fs.existsSync(path.join(owned,name)),false,'failed isolation must not enter the installation Section');
      }
      assert.equal(registration.hostFixtureKeysAbsent,true);
      if(name==='ok') {
        const bytes=fs.readFileSync(receipt);
        assert.deepEqual([...bytes.subarray(0,2)],[0xff,0xfe]);
        assert.equal(bytes.length%2,0);
        assert.deepEqual(bytes.subarray(2).toString('utf16le').split('\r\n'),['grok-nsis-install-complete-v1',nonce,'1.2.3',product,'owned-main.exe','com.grokapp.completion-fixture',path.join(owned,name),String(native.pid),native.created,'complete','']);
        fs.writeFileSync(path.join(dir,'native-callback.json'),JSON.stringify({nonce,product,version:'1.2.3',bundleId:'com.grokapp.completion-fixture',executable:path.join(owned,name,'owned-main.exe'),receipt,receiptSha256:hash(receipt),process:native},null,2)+'\n');
      } else if(name==='existing') assert.deepEqual(fs.readFileSync(receipt),sentinel);
      else assert.equal(fs.existsSync(receipt),false,'rejected callback must not publish completion');
      observations.push({case:name,process:native,registration,receiptPresent:fs.existsSync(receipt)});
      fs.writeFileSync(path.join(dir,'observations.json'),JSON.stringify(observations,null,2)+'\n');
    });
  }
  assert.equal(observations.length,cases.length,'every native case must succeed before sealing a receipt');
  assert.equal(inspect().hostFixtureKeysAbsent,true);
  const report={scope:'Real NSIS success callback and native registry read/write in process-private application hives; NOT signed Grok installer, updater lifecycle or installed UI acceptance',registryView:64,hookSha256:hash(hook),fixtureSourceSha256:hash(path.join(dir,'fixture.nsi')),fixtureBinarySha256:hash(path.join(dir,'owned-private-callback.exe')),casesPassed:observations.length,hostFixtureKeysAbsent:true,productionInstallerExecuted:false,installedAppExecuted:false,signedInstallationAcceptance:false};
  fs.writeFileSync(path.join(dir,'private-callback-receipt.json'),JSON.stringify(report,null,2)+'\n');
  t.diagnostic(JSON.stringify({...report,evidenceDirectory:parent?dir:undefined}));
});
