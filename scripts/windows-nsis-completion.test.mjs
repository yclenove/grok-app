import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createHash, randomUUID } from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const hook = path.join(repo, 'src-tauri/nsis/update-completion.nsh');
const template = fs.readFileSync(path.join(repo, 'src-tauri/nsis/installer.nsi'), 'utf8');

function compiler(t) {
  const nsis = [process.env.GROK_MAKENSIS,
    process.env.LOCALAPPDATA && path.join(process.env.LOCALAPPDATA, 'tauri/NSIS/makensis.exe'),
    'C:/Program Files (x86)/NSIS/makensis.exe', 'C:/Program Files/NSIS/makensis.exe',
  ].find((candidate) => candidate && fs.existsSync(candidate));
  if (process.platform === 'win32' && nsis) return nsis;
  assert.notEqual(process.env.GROK_NSIS_REQUIRED, '1', 'required Windows NSIS compiler unavailable');
  t.skip('requires Windows NSIS; mandatory in the Windows CI job');
  return undefined;
}

test('publisher completion declaration is wired to the current-user success callback', () => {
  const config = JSON.parse(fs.readFileSync(path.join(repo, 'src-tauri/tauri.conf.json'), 'utf8'));
  assert.equal(config.bundle.windows.nsis.installMode, 'currentUser');
  assert.equal(config.bundle.windows.nsis.installerHooks, 'nsis/update-completion.nsh');
  assert.equal(config.bundle.windows.nsis.template, 'nsis/installer.nsi');
  assert.match(template, /!include "{{installer_hooks}}"/);
  assert.match(template.replace(/^\s*;.*$/gm, ''), /!insertmacro GROK_DEFINE_UPDATE_COMPLETION\s+Function \.onInstSuccess\s+Call GrokWriteUpdateCompletion/);
  const callback = fs.readFileSync(hook, 'utf8');
  assert.doesNotMatch(callback, /\b(?:WriteReg\w*|DeleteReg\w*|Exec\w*|Delete|RMDir)\b/);
  assert.match(callback, /CreateFileW\([^\n]*i 1, p 0, i 1, i 0x80200080/);
  assert.match(callback, /IfFileExists "\$INSTDIR\\uninstall\.exe" 0 grok_complete_done\s+!insertmacro GROK_WRITE_UPDATE_COMPLETION_RECEIPT grok_complete_done/);
  assert.equal((callback.match(/!insertmacro GROK_WRITE_UPDATE_COMPLETION_RECEIPT/g) || []).length, 1);
});

test('production NSIS completion macro compiles without running an installer', (t) => {
  const nsis = compiler(t);
  if (!nsis) return;
  const evidenceParent = process.env.GROK_NSIS_TEST_ARTIFACT_DIR;
  const dir = fs.mkdtempSync(path.join(evidenceParent || os.tmpdir(), 'grok-nsis-completion-'));
  if (!evidenceParent) t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  const escape = (s) => s.replaceAll('$', () => '$$').replaceAll('"', '$\\"');
  // Compile-only. This private inert fixture has no registry writes, shortcuts,
  // App launch, uninstaller launch, or actual installation acceptance claim.
  fs.writeFileSync(path.join(dir, 'main.bin'), 'MZ inert compile fixture; never execute');
  const source = `Unicode true
!include "LogicLib.nsh"
!include "FileFunc.nsh"
!include "StrFunc.nsh"
\${StrLoc}
!define INSTALLMODE "currentUser"
!define PRODUCTNAME "Grok Completion Fixture"
!define MANUFACTURER "Owned Fixture"
!define VERSION "1.2.3"
!define MAINBINARYNAME "grok-fixture"
!define BUNDLEID "com.grokapp.completion-fixture"
!define UNINSTKEY "Software\\GrokCompletionFixture\\Uninstall"
!define MANUPRODUCTKEY "Software\\GrokCompletionFixture"
!include "${escape(hook)}"
Name "Owned completion compile fixture"
OutFile "owned-completion-setup.exe"
RequestExecutionLevel user
Var UpdateMode
!insertmacro GROK_DEFINE_UPDATE_COMPLETION
Section
  StrCpy $UpdateMode 1
  SetOutPath "$INSTDIR"
  File /oname=grok-fixture.exe "main.bin"
SectionEnd
Function .onInstSuccess
  Call GrokWriteUpdateCompletion
FunctionEnd
`;
  fs.writeFileSync(path.join(dir, 'fixture.nsi'), '\uFEFF' + source);
  let output;
  try {
    output = execFileSync(nsis, ['/V4', 'fixture.nsi'], {
      cwd: dir, encoding: 'utf8', timeout: 60000, maxBuffer: 4 * 1024 * 1024,
    });
  } catch (error) {
    fs.writeFileSync(path.join(dir, 'compile.log'), String(error.stdout || '') + String(error.stderr || ''));
    throw error;
  }
  fs.writeFileSync(path.join(dir, 'compile.log'), output);
  const binary = fs.readFileSync(path.join(dir, 'owned-completion-setup.exe'));
  assert.equal(binary.subarray(0, 2).toString(), 'MZ');
  assert.doesNotMatch(output, /warning \d+/i);
  const receipt = {
    compiler: nsis, sourceSha256: createHash('sha256').update(source).digest('hex'),
    hookSha256: createHash('sha256').update(fs.readFileSync(hook)).digest('hex'),
    binarySha256: createHash('sha256').update(binary).digest('hex'),
    installerExecuted: false, registryChanged: false, signedInstallationAcceptance: false,
  };
  fs.writeFileSync(path.join(dir, 'compile-receipt.json'), JSON.stringify(receipt, null, 2) + '\n');
  t.diagnostic(JSON.stringify({ ...receipt, evidenceDirectory: evidenceParent ? dir : undefined }));
});

test('shared production writer runs in a registry-free non-installing native fixture', async (t) => {
  const nsis = compiler(t);
  if (!nsis) return;
  const evidenceParent = process.env.GROK_NSIS_TEST_ARTIFACT_DIR;
  const dir = fs.mkdtempSync(path.join(evidenceParent || os.tmpdir(), 'grok-receipt-writer-'));
  if (!evidenceParent) t.after(() => fs.rmSync(dir, {recursive:true, force:true}));
  const owned = path.join(dir, 'native 空格 $ owned');
  fs.mkdirSync(owned);
  const nonce = randomUUID();
  const product = 'Owned 回执 🧪';
  const escape = s => s.replaceAll('$', () => '$$').replaceAll('"', '$\\"');
  // This is NOT the production installer or its registration gate. It runs
  // only the exact shared native writer, then exits before the empty Section.
  // No registry operations, installed App, shortcuts or uninstaller exist.
  const source = `Unicode true
!include "FileFunc.nsh"
!define PRODUCTNAME "${escape(product)}"
!define VERSION "1.2.3"
!define MAINBINARYNAME "owned-main"
!define BUNDLEID "com.grokapp.completion-fixture"
!include "${escape(hook)}"
Name "Owned non-installing receipt writer"
OutFile "owned-receipt-writer.exe"
RequestExecutionLevel user
SilentInstall silent
AutoCloseWindow true
Var GrokUpdateId
Var GrokUpdateReceipt
Function .onInit
  StrCpy $INSTDIR "${escape(owned)}"
  StrCpy $GrokUpdateId "${nonce}"
  ClearErrors
  \${GetOptions} $CMDLINE "/CASE=" $R0
  IfErrors refused
  StrCmp $R0 "ok" allowed
  StrCmp $R0 "existing" allowed
  StrCmp $R0 "hardlink" allowed
  StrCmp $R0 "directory" allowed
  StrCmp $R0 "missing" allowed refused
  refused:
    SetErrorLevel 70
    Quit
  allowed:
    StrCpy $R1 0
  wait_for_owner:
    IfFileExists "$INSTDIR\\permit-$R0" run_writer
    IntOp $R1 $R1 + 1
    IntCmp $R1 500 timed_out
    Sleep 20
    Goto wait_for_owner
  timed_out:
    SetErrorLevel 71
    Quit
  run_writer:
    StrCpy $GrokUpdateReceipt "$INSTDIR\\$R0.receipt"
    StrCmp $R0 "missing" 0 +2
    StrCpy $GrokUpdateReceipt "$INSTDIR\\absent\\missing.receipt"
    !insertmacro GROK_WRITE_UPDATE_COMPLETION_RECEIPT writer_done
  writer_done:
    SetErrorLevel 0
    Quit
FunctionEnd
Section
SectionEnd
`;
  assert.doesNotMatch(source, /\b(?:WriteReg\w*|DeleteReg\w*|ReadReg\w*|Exec\w*|Delete|RMDir|SetOutPath|WriteUninstaller)\b/);
  fs.writeFileSync(path.join(dir,'writer.nsi'), '\uFEFF'+source);
  let output;
  try {
    output = execFileSync(nsis, ['/V4','writer.nsi'], {cwd:dir,encoding:'utf8',timeout:60000,maxBuffer:4*1024*1024});
  } catch(error) {
    fs.writeFileSync(path.join(dir,'compile.log'),String(error.stdout||'')+String(error.stderr||''));
    throw error;
  }
  fs.writeFileSync(path.join(dir,'compile.log'),output);
  assert.doesNotMatch(output,/warning \d+/i);
  // Hold the exact process handle and read FILETIME before authorizing this
  // owned child. Never find/kill another process by name or an untrusted PID.
  const runner = `param([string]$Binary,[string]$Owned,[ValidateSet('ok','existing','hardlink','directory','missing')][string]$Case)
$ErrorActionPreference='Stop'
$permit=Join-Path $Owned "permit-$Case"
if(Test-Path -LiteralPath $permit){throw 'Refusing to reuse a fixture permit'}
$p=[System.Diagnostics.Process]::new()
$p.StartInfo.FileName=$Binary
$p.StartInfo.WorkingDirectory=$Owned
$p.StartInfo.Arguments="/CASE=$Case"
$p.StartInfo.UseShellExecute=$false
try {
  if(-not $p.Start()){throw 'Owned fixture failed to start'}
  $handle=$p.Handle
  $created=$p.StartTime.ToUniversalTime().ToFileTimeUtc().ToString([Globalization.CultureInfo]::InvariantCulture)
  $identity=$p.Id
  [System.IO.File]::WriteAllText($permit,'owned test permit')
  if(-not $p.WaitForExit(15000)){$p.Kill();$p.WaitForExit();throw 'Owned receipt fixture exceeded its deadline'}
  @{pid=$identity;created=$created;exitCode=$p.ExitCode;handleHeld=($handle -ne [IntPtr]::Zero)}|ConvertTo-Json -Compress
} finally {
  if($handle -and -not $p.HasExited){$p.Kill();$p.WaitForExit()}
  $p.Dispose()
}
`;
  const runnerPath = path.join(dir,'run-owned.ps1');
  fs.writeFileSync(runnerPath,'\uFEFF'+runner);
  const observations = [];
  let verifiedCases = 0;
  function run(name) {
    const result = JSON.parse(execFileSync('powershell.exe', ['-NoProfile','-NonInteractive','-File',runnerPath,'-Binary',path.join(dir,'owned-receipt-writer.exe'),'-Owned',owned,'-Case',name], {encoding:'utf8',timeout:30000,maxBuffer:1024*1024}).replace(/^\uFEFF/,''));
    assert.equal(result.exitCode,0);
    assert.equal(result.handleHeld,true);
    observations.push({case:name,...result});
    fs.writeFileSync(path.join(dir,`${name}-process.json`),JSON.stringify(result,null,2)+'\n');
    return result;
  }
  await t.test('writes exact UTF-16LE fields, Unicode path and native PID/creation time', () => {
    const native = run('ok');
    const bytes = fs.readFileSync(path.join(owned,'ok.receipt'));
    assert.deepEqual([...bytes.subarray(0,2)],[0xff,0xfe]);
    assert.equal(bytes.length%2,0);
    assert(BigInt(native.created)>0n);
    assert.deepEqual(bytes.subarray(2).toString('utf16le').split('\r\n'),[
      'grok-nsis-install-complete-v1',nonce,'1.2.3',product,'owned-main.exe',
      'com.grokapp.completion-fixture',owned,String(native.pid),native.created,'complete','',
    ]);
    verifiedCases += 1;
  });
  for(const kind of ['existing','hardlink','directory','missing']) {
    await t.test(`does not overwrite or create through ${kind} destination`, () => {
      const dest=path.join(owned,`${kind}.receipt`);
      const sentinel=Buffer.from(`untouched ${nonce} ${kind}`);
      const linkTarget=path.join(owned,'hardlink-target');
      if(kind==='existing') fs.writeFileSync(dest,sentinel);
      if(kind==='hardlink') {fs.writeFileSync(linkTarget,sentinel);fs.linkSync(linkTarget,dest);}
      if(kind==='directory') fs.mkdirSync(dest);
      run(kind);
      if(kind==='existing'||kind==='hardlink') assert.deepEqual(fs.readFileSync(dest),sentinel);
      if(kind==='hardlink') {assert.deepEqual(fs.readFileSync(linkTarget),sentinel);assert.equal(fs.statSync(linkTarget).nlink,2);}
      if(kind==='directory') assert.deepEqual(fs.readdirSync(dest),[]);
      if(kind==='missing') assert.equal(fs.existsSync(path.join(owned,'absent')),false);
      verifiedCases += 1;
    });
  }
  assert.equal(verifiedCases,5,'all native writer assertions must pass before sealing a receipt');
  const report = {scope:'Exact shared native writer only; not production registration gate or installation acceptance',hookSha256:createHash('sha256').update(fs.readFileSync(hook)).digest('hex'),binarySha256:createHash('sha256').update(fs.readFileSync(path.join(dir,'owned-receipt-writer.exe'))).digest('hex'),observations,productionInstallerExecuted:false,registryChanged:false,signedInstallationAcceptance:false};
  fs.writeFileSync(path.join(dir,'runtime-receipt.json'),JSON.stringify(report,null,2)+'\n');
  t.diagnostic(JSON.stringify({...report,evidenceDirectory:evidenceParent?dir:undefined}));
});
