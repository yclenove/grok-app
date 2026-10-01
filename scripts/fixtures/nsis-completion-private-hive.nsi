; Non-production, inert fixture. The production callback is included unchanged.
; All registration writes MUST remain after the process-private HKCU fence.
Unicode true
!include "LogicLib.nsh"
!include "FileFunc.nsh"
!include "StrFunc.nsh"
${StrLoc}
!include "fixture-defines.nsh"
!include "${COMPLETION_HOOK}"
!define INSTALLMODE "currentUser"
!define VERSION "1.2.3"
!define MAINBINARYNAME "owned-main"
!define MANUFACTURER "Owned callback fixture"
!define BUNDLEID "com.grokapp.completion-fixture"
Name "Owned private-hive callback fixture"
OutFile "owned-private-callback.exe"
RequestExecutionLevel user
SilentInstall silent
AutoCloseWindow true
Var UpdateMode
Var FixtureCase
Var PrivateHive
Var PrivateRoot
Var FenceHandle
!insertmacro GROK_DEFINE_UPDATE_COMPLETION

Function .onInit
  ClearErrors
  ${GetOptions} $CMDLINE "/CASE=" $FixtureCase
  IfErrors refuse
  StrCmp $FixtureCase "ok" allowed
  StrCmp $FixtureCase "bad-product" allowed
  StrCmp $FixtureCase "bad-version" allowed
  StrCmp $FixtureCase "bad-publisher" allowed
  StrCmp $FixtureCase "bad-main" allowed
  StrCmp $FixtureCase "bad-location" allowed
  StrCmp $FixtureCase "bad-uninstall" allowed
  StrCmp $FixtureCase "bad-root" allowed
  StrCmp $FixtureCase "no-main" allowed
  StrCmp $FixtureCase "no-uninstaller" allowed
  StrCmp $FixtureCase "not-update" allowed
  StrCmp $FixtureCase "bad-nonce" allowed
  StrCmp $FixtureCase "uppercase-nonce" allowed
  StrCmp $FixtureCase "bad-variant" allowed
  StrCmp $FixtureCase "missing-receipt" allowed
  StrCmp $FixtureCase "short-nonce" allowed
  StrCmp $FixtureCase "nonhex-nonce" allowed
  StrCmp $FixtureCase "missing-name" allowed
  StrCmp $FixtureCase "existing" allowed
  StrCmp $FixtureCase "hive-failure" allowed
  StrCmp $FixtureCase "override-failure" allowed refuse
  refuse:
    SetErrorLevel 70
    Quit
  allowed:
    StrCpy $INSTDIR "${FIXTURE_ROOT}\$FixtureCase"
    SetShellVarContext current
    StrCpy $UpdateMode 1
    StrCmp $FixtureCase "not-update" 0 +2
    StrCpy $UpdateMode 0
    ; The owner holds this exact process's handle before releasing the permit.
    StrCpy $R1 0
  wait_for_owner:
    IfFileExists "${FIXTURE_ROOT}\permit-$FixtureCase" isolate
    IntOp $R1 $R1 + 1
    IntCmp $R1 500 timed_out
    Sleep 20
    Goto wait_for_owner
  timed_out:
    SetErrorLevel 71
    Quit
  isolate:
    StrCpy $R2 "preexisting-hive"
    StrCpy $R0 "${FIXTURE_ROOT}\$FixtureCase.hiv"
    IfFileExists "$R0" isolation_failed
    StrCmp $FixtureCase "hive-failure" 0 +2
    StrCpy $R0 "${FIXTURE_ROOT}\absent\failure.hiv"
    ; REG_PROCESS_APPKEY, KEY_ALL_ACCESS; no namespace mount, no privilege enable.
    StrCpy $R2 "load-hive"
    System::Call 'advapi32::RegLoadAppKeyW(w "$R0", *p .r8, i 0xF003F, i 1, i 0) i.r9'
    StrCmp $9 0 0 isolation_failed
    StrCpy $PrivateHive $8
    StrCpy $R2 "create-root"
    System::Call 'advapi32::RegCreateKeyExW(p r8, w "OwnedRoot", i 0, p 0, i 0, i 0xF003F, p 0, *p .r7, *i .r6) i.r9'
    StrCmp $9 0 0 isolation_failed
    StrCpy $PrivateRoot $7
    StrCpy $R2 "create-fence"
    ; This key is created by explicit private handle, never by predefined HKCU.
    System::Call 'advapi32::RegCreateKeyExW(p r7, w "${FENCE}", i 0, p 0, i 0, i 0xF003F, p 0, *p .r8, *i .r6) i.r9'
    StrCmp $9 0 0 isolation_failed
    StrCpy $FenceHandle $8
    StrCmp $FixtureCase "override-failure" 0 +2
    StrCpy $7 0xBADBEEF
    StrCpy $R2 "override"
    System::Call 'advapi32::RegOverridePredefKey(p 0x80000001, p r7) i.r9'
    StrCmp $9 0 0 isolation_failed
    ; Prove the override reaches the unique private fence BEFORE WriteRegStr.
    StrCpy $R2 "verify-fence"
    System::Call 'advapi32::RegOpenKeyExW(p 0x80000001, w "${FENCE}", i 0, i 0x20119, *p .r8) i.r9'
    StrCmp $9 0 0 isolation_failed
    System::Call 'advapi32::RegCloseKey(p r8)'
    ; Match the production x64 SetContext macro, after proving the private fence.
    SetRegView 64
    Return
  isolation_failed:
    FileOpen $0 "${FIXTURE_ROOT}\$FixtureCase.isolation-error" w
    FileWriteUTF16LE /BOM $0 "$R2$\r$\n$9$\r$\n"
    FileClose $0
    SetErrorLevel 73
    Quit
FunctionEnd

Section
  ; Inert files in this UUID-owned fixture only; they are NEVER executed.
  SetOutPath "$INSTDIR"
  StrCmp $FixtureCase "no-main" +2
  File /oname=owned-main.exe "inert.bin"
  StrCmp $FixtureCase "no-uninstaller" +2
  File /oname=uninstall.exe "inert.bin"
  ClearErrors
  StrCmp $FixtureCase "missing-name" +2
  WriteRegStr SHCTX "${UNINSTKEY}" "DisplayName" "${PRODUCTNAME}"
  WriteRegStr SHCTX "${UNINSTKEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr SHCTX "${UNINSTKEY}" "Publisher" "${MANUFACTURER}"
  WriteRegStr SHCTX "${UNINSTKEY}" "MainBinaryName" "${MAINBINARYNAME}.exe"
  WriteRegStr SHCTX "${UNINSTKEY}" "InstallLocation" '$\"$INSTDIR$\"'
  WriteRegStr SHCTX "${UNINSTKEY}" "UninstallString" '$\"$INSTDIR\uninstall.exe$\"'
  WriteRegStr SHCTX "${MANUPRODUCTKEY}" "" "$INSTDIR"
  IfErrors registration_failed
  StrCmp $FixtureCase "bad-product" 0 +2
  WriteRegStr SHCTX "${UNINSTKEY}" "DisplayName" "different"
  StrCmp $FixtureCase "bad-version" 0 +2
  WriteRegStr SHCTX "${UNINSTKEY}" "DisplayVersion" "0.0.0"
  StrCmp $FixtureCase "bad-publisher" 0 +2
  WriteRegStr SHCTX "${UNINSTKEY}" "Publisher" "different"
  StrCmp $FixtureCase "bad-main" 0 +2
  WriteRegStr SHCTX "${UNINSTKEY}" "MainBinaryName" "different.exe"
  StrCmp $FixtureCase "bad-location" 0 +2
  WriteRegStr SHCTX "${UNINSTKEY}" "InstallLocation" "$INSTDIR"
  StrCmp $FixtureCase "bad-uninstall" 0 +2
  WriteRegStr SHCTX "${UNINSTKEY}" "UninstallString" "different"
  StrCmp $FixtureCase "bad-root" 0 +2
  WriteRegStr SHCTX "${MANUPRODUCTKEY}" "" "different"
  Goto section_done
  registration_failed:
    SetErrorLevel 74
    Quit
  section_done:
SectionEnd

Function .onInstSuccess
  Call GrokWriteUpdateCompletion
  ; Observation marker proves the success callback ran, not that it approved.
  FileOpen $0 "${FIXTURE_ROOT}\$FixtureCase.callback-ran" w
  FileWrite $0 "callback returned"
  FileClose $0
  System::Call 'advapi32::RegFlushKey(p $PrivateHive) i.r9'
  StrCmp $9 0 0 flush_failed
  ; Keep the override alive until process exit. Do not restore host HKCU and
  ; then run a later callback. Windows closes/unloads these private handles.
  SetErrorLevel 0
  Return
  flush_failed:
    SetErrorLevel 75
FunctionEnd
