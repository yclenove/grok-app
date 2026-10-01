; Registry-free, inert fixture: invokes the real NSIS/MUI failure callbacks.
Unicode true
RequestExecutionLevel user
SilentInstall normal
!include fixture-defines.nsh
!define MUI_CUSTOMFUNCTION_ABORT GrokReportUpdateCancelled
!include MUI2.nsh
!include FileFunc.nsh
!include LogicLib.nsh
!include StrFunc.nsh
${StrLoc}
!define VERSION "1.2.3"
!define MAINBINARYNAME "owned-main"
!define BUNDLEID "com.grokapp.failure-fixture"
!define INSTALLMODE "currentUser"
!include "${COMPLETION_HOOK}"
Name "${PRODUCTNAME}"
OutFile "owned-failure-callback.exe"
Var UpdateMode
Var GrokUpdateId
Var Case
!define MUI_PAGE_CUSTOMFUNCTION_SHOW FixtureShown
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"
!insertmacro GROK_DEFINE_UPDATE_FAILURE

Function .onInit
  SetSilent silent
  ${GetOptions} $CMDLINE "/CASE=" $Case
  StrCpy $UpdateMode 1
  StrCmp $Case "not-update" 0 +2
    StrCpy $UpdateMode 0
  StrCpy $INSTDIR "${FIXTURE_ROOT}\$Case"
  ; Wait for the parent to retain this exact process handle before any callback.
  StrCpy $0 0
  wait_permit:
    IfFileExists "${FIXTURE_ROOT}\permit-$Case" permitted
    Sleep 10
    IntOp $0 $0 + 1
    IntCmp $0 1000 permit_timeout wait_permit permit_timeout
  permit_timeout:
    SetErrorLevel 73
    Quit
  permitted:
  StrCmp $Case "cancelled" 0 +2
    SetSilent normal
FunctionEnd

Function FixtureShown
  ; The page's nested message loop must exist before it can accept cancellation.
  ${NSD_CreateTimer} FixtureCancel 100
FunctionEnd

Function FixtureCancel
  ${NSD_KillTimer} FixtureCancel
  ; Deliver Cancel to this fixture's own window. No other app/window is touched.
  GetDlgItem $1 $HWNDPARENT 2
  System::Call 'user32::IsWindowEnabled(p r1) i.r0'
  FileOpen $9 "${FIXTURE_ROOT}\$Case.gui-message" w
  FileWrite $9 "window=$HWNDPARENT cancel=$1 enabled=$0"
  FileClose $9
  SendMessage $HWNDPARENT ${WM_COMMAND} 2 0
FunctionEnd

Function .onInstFailed
  FileOpen $0 "${FIXTURE_ROOT}\$Case.callback-ran" w
  FileWrite $0 "actual .onInstFailed"
  FileClose $0
  Call GrokReportUpdateFailed
FunctionEnd

Section
  ; No payload, registration, services, uninstaller, or application are executed.
  Abort "Owned deliberate Section failure"
SectionEnd
