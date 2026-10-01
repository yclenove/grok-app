import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash,randomUUID} from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import test from 'node:test';

const repo=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const fixtures=path.join(repo,'scripts/fixtures');
const hook=path.join(repo,'src-tauri/nsis/update-completion.nsh');
const hash=p=>createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const escape=s=>s.replaceAll('$',()=>'$$').replaceAll('"','$\\"');

test('production failure and confirmed-cancel hooks are distinct from completion',()=>{
  const installer=fs.readFileSync(path.join(repo,'src-tauri/nsis/installer.nsi'),'utf8').replace(/^\s*;.*$/gm,'');
  assert.match(installer,/!define MUI_CUSTOMFUNCTION_ABORT GrokReportUpdateCancelled\s+!include MUI2.nsh/);
  assert.match(installer,/!insertmacro GROK_DEFINE_UPDATE_FAILURE\s+Function \.onInstFailed\s+Call GrokReportUpdateFailed/);
  assert.doesNotMatch(installer,/Function \.onUserAbort/,'Modern UI owns its abort callback');
  const failure=fs.readFileSync(hook,'utf8').split('!macro GROK_DEFINE_UPDATE_FAILURE')[1];
  assert.doesNotMatch(failure,/\b(?:WriteReg\w*|DeleteReg\w*|ReadReg\w*|Exec\w*|Delete|RMDir)\b/);
  assert.match(failure,/grok-nsis-install-failed-v1/);
  assert.match(failure,/GROK_VALIDATE_UPDATE_NONCE grok_failure/);
});

test('real NSIS failed Section and MUI user-abort publish only owned failure receipts',async t=>{
  const nsis=[process.env.GROK_MAKENSIS,process.env.LOCALAPPDATA&&path.join(process.env.LOCALAPPDATA,'tauri/NSIS/makensis.exe'),'C:/Program Files (x86)/NSIS/makensis.exe'].find(p=>p&&fs.existsSync(p));
  if(process.platform!=='win32'||!nsis){assert.notEqual(process.env.GROK_NSIS_REQUIRED,'1','required Windows NSIS unavailable');return t.skip('Windows NSIS required');}
  const parent=process.env.GROK_NSIS_TEST_ARTIFACT_DIR;
  const dir=fs.mkdtempSync(path.join(parent||os.tmpdir(),'grok-failure-callback-'));
  if(!parent)t.after(()=>fs.rmSync(dir,{recursive:true,force:true}));
  const owned=path.join(dir,'owned 空格 $ 🙂');fs.mkdirSync(owned);
  const nonce='a'+randomUUID().slice(1),product=`Owned 失败 ${nonce}`;
  const defines={COMPLETION_HOOK:hook,FIXTURE_ROOT:owned,PRODUCTNAME:product};
  fs.writeFileSync(path.join(dir,'fixture-defines.nsh'),'\uFEFF'+Object.entries(defines).map(([k,v])=>`!define ${k} "${escape(v)}"`).join('\n')+'\n');
  const source=fs.readFileSync(path.join(fixtures,'nsis-failure-callback.nsi'),'utf8');
  assert.doesNotMatch(source,/\b(?:WriteReg\w*|ReadReg\w*|DeleteReg\w*|Exec\w*|Delete|RMDir|WriteUninstaller)\b/);
  assert.match(source,/SendMessage \$HWNDPARENT \$\{WM_COMMAND\} 2 0/);
  fs.writeFileSync(path.join(dir,'fixture.nsi'),'\uFEFF'+source);
  let compiled;
  try{compiled=execFileSync(nsis,['/V4','fixture.nsi'],{cwd:dir,encoding:'utf8',timeout:60000,maxBuffer:4*1024*1024});}
  catch(e){fs.writeFileSync(path.join(dir,'compile.log'),String(e.stdout||'')+String(e.stderr||''));throw e;}
  fs.writeFileSync(path.join(dir,'compile.log'),compiled);assert.doesNotMatch(compiled,/warning \d+/i);
  const observations=[];
  for(const name of ['failed','cancelled','not-update','uppercase-nonce','bad-version','bad-variant','missing-receipt','existing','hardlink','directory']) {
    await t.test(name,()=>{
      const receipt=path.join(owned,`${name}.receipt`),sentinel=Buffer.from(`untouched ${nonce}`);
      if(name==='existing')fs.writeFileSync(receipt,sentinel);
      if(name==='hardlink'){fs.writeFileSync(path.join(owned,'link-source'),sentinel);fs.linkSync(path.join(owned,'link-source'),receipt);}
      if(name==='directory')fs.mkdirSync(receipt);
      let id=nonce;
      if(name==='uppercase-nonce')id=id.toUpperCase();
      if(name==='bad-version')id=id.slice(0,14)+'1'+id.slice(15);
      if(name==='bad-variant')id=id.slice(0,19)+'0'+id.slice(20);
      const native=JSON.parse(execFileSync('powershell.exe',['-NoProfile','-NonInteractive','-File',path.join(fixtures,'run-failure-nsis.ps1'),'-Binary',path.join(dir,'owned-failure-callback.exe'),'-Owned',owned,'-Case',name,'-Nonce',id],{encoding:'utf8',timeout:30000,maxBuffer:1024*1024}).replace(/^\uFEFF/,''));
      fs.writeFileSync(path.join(dir,`${name}-process.json`),JSON.stringify(native,null,2)+'\n');
      assert.equal(native.handleHeld,true);assert(BigInt(native.exited)>BigInt(native.created));
      assert.equal(native.exitCode,name==='cancelled'?1:2);
      assert.equal(fs.existsSync(path.join(owned,`${name}.callback-ran`)),name!=='cancelled');
      if(name==='failed'||name==='cancelled'){
        const bytes=fs.readFileSync(receipt);assert.deepEqual([...bytes.subarray(0,2)],[255,254]);
        assert.deepEqual(bytes.subarray(2).toString('utf16le').split('\r\n'),['grok-nsis-install-failed-v1',nonce,'1.2.3',product,'owned-main.exe','com.grokapp.failure-fixture',path.join(owned,name),String(native.pid),native.created,name,'']);
        fs.writeFileSync(path.join(dir,`native-${name}-callback.json`),JSON.stringify({nonce,product,version:'1.2.3',bundleId:'com.grokapp.failure-fixture',executable:path.join(owned,name,'owned-main.exe'),receipt,receiptSha256:hash(receipt),outcome:name,process:native},null,2)+'\n');
      }else if(name==='existing'||name==='hardlink')assert.deepEqual(fs.readFileSync(receipt),sentinel);
      else if(name==='directory')assert(fs.statSync(receipt).isDirectory());
      else assert.equal(fs.existsSync(receipt),false);
      observations.push({case:name,process:native,receiptPresent:fs.existsSync(receipt)});
      fs.writeFileSync(path.join(dir,'observations.json'),JSON.stringify(observations,null,2)+'\n');
    });
  }
  assert.equal(observations.length,10);
  fs.writeFileSync(path.join(dir,'receipt.json'),JSON.stringify({scope:'inert registry-free actual NSIS failure/MUI abort; NOT installed App or rollback proof',hookSha256:hash(hook),fixtureSha256:hash(path.join(fixtures,'nsis-failure-callback.nsi')),compilerSha256:hash(nsis),cases:observations.length},null,2)+'\n');
});
