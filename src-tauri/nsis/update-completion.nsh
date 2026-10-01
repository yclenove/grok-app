!macro GROK_VALIDATE_UPDATE_NONCE prefix done_label
  StrLen $0 $GrokUpdateId
  StrCmp $0 36 0 ${done_label}
  StrCpy $0 0
  ${prefix}_nonce_loop:
    StrCpy $1 $GrokUpdateId 1 $0
    ${If} $0 = 8
    ${OrIf} $0 = 13
    ${OrIf} $0 = 18
    ${OrIf} $0 = 23
      StrCmp $1 "-" 0 ${done_label}
    ${Else}
      ${StrLoc} $2 "0123456789abcdef" $1 ">"
      StrCmp $2 "" ${done_label}
      ; StrLoc/StrCmp ignore case. The signed/native nonce contract does not.
      StrCpy $3 "0123456789abcdef" 1 $2
      StrCmpS $1 $3 0 ${done_label}
    ${EndIf}
    IntOp $0 $0 + 1
    IntCmp $0 36 ${prefix}_nonce_ok ${prefix}_nonce_loop ${done_label}
  ${prefix}_nonce_ok:
  StrCpy $1 $GrokUpdateId 1 14
  StrCmp $1 "4" 0 ${done_label}
  StrCpy $1 $GrokUpdateId 1 19
  ${StrLoc} $2 "89ab" $1 ">"
  StrCmp $2 "" ${done_label}

!macroend

; Definitions are expanded after installer.nsi defines its product constants.
; Neither callback changes registration. Success is published only from
; .onInstSuccess after registration readback; failure/cancel is diagnostic only.
; Both use nonce-bound CREATE_NEW receipts and exact native process identity.
; Keep the native writer shared with the non-installing receipt fixture. The
; production call site below must still pass every registration/nonce check.
!macro GROK_WRITE_UPDATE_COMPLETION_RECEIPT done_label
  System::Call 'kernel32::GetCurrentProcess() p.r1'
  System::Call 'kernel32::GetProcessTimes(p r1, *l .r2, *l .r3, *l .r4, *l .r5) i.r6'
  StrCmp $6 0 ${done_label}
  System::Call 'kernel32::GetCurrentProcessId() i.r3'
  ; CREATE_NEW + FILE_SHARE_READ + WRITE_THROUGH + OPEN_REPARSE_POINT.
  ; FileWriteUTF16LE uses the native file handle, just like NSIS FileOpen.
  System::Call 'kernel32::CreateFileW(w "$GrokUpdateReceipt", i 0x40000000, i 1, p 0, i 1, i 0x80200080, p 0) p.r0'
  StrCmp $0 -1 ${done_label}
  ClearErrors
  FileWriteUTF16LE /BOM $0 "grok-nsis-install-complete-v1$\r$\n$GrokUpdateId$\r$\n${VERSION}$\r$\n${PRODUCTNAME}$\r$\n${MAINBINARYNAME}.exe$\r$\n${BUNDLEID}$\r$\n$INSTDIR$\r$\n$3$\r$\n$2$\r$\ncomplete$\r$\n"
  System::Call 'kernel32::FlushFileBuffers(p r0) i.r1'
  System::Call 'kernel32::CloseHandle(p r0)'
!macroend

!macro GROK_DEFINE_UPDATE_COMPLETION
Var GrokUpdateId
Var GrokUpdateReceipt
Function GrokWriteUpdateCompletion
  Push $0
  Push $1
  Push $2
  Push $3
  Push $4
  Push $5
  Push $6
  Push $7
  Push $8
  Push $9
  ${If} $UpdateMode != 1
    Goto grok_complete_done
  ${EndIf}
  !if "${INSTALLMODE}" != "currentUser"
    Goto grok_complete_done
  !endif
  ClearErrors
  ${GetOptions} $CMDLINE "/GROKUPDATEID=" $GrokUpdateId
  IfErrors grok_complete_done
  ${GetOptions} $CMDLINE "/GROKUPDATERECEIPT=" $GrokUpdateReceipt
  IfErrors grok_complete_done
  StrCmp $GrokUpdateReceipt "" grok_complete_done
  !insertmacro GROK_VALIDATE_UPDATE_NONCE grok_complete grok_complete_done

  ; These are the values written by this signed installer, not inferred from
  ; an EXE version or process exit. Failed/partial registration has no receipt.
  ReadRegStr $0 SHCTX "${UNINSTKEY}" "DisplayName"
  StrCmp $0 "${PRODUCTNAME}" 0 grok_complete_done
  ReadRegStr $0 SHCTX "${UNINSTKEY}" "DisplayVersion"
  StrCmp $0 "${VERSION}" 0 grok_complete_done
  ReadRegStr $0 SHCTX "${UNINSTKEY}" "Publisher"
  StrCmp $0 "${MANUFACTURER}" 0 grok_complete_done
  ReadRegStr $0 SHCTX "${UNINSTKEY}" "MainBinaryName"
  StrCmp $0 "${MAINBINARYNAME}.exe" 0 grok_complete_done
  ReadRegStr $0 SHCTX "${UNINSTKEY}" "InstallLocation"
  StrCmp $0 '$\"$INSTDIR$\"' 0 grok_complete_done
  ReadRegStr $0 SHCTX "${UNINSTKEY}" "UninstallString"
  StrCmp $0 '$\"$INSTDIR\uninstall.exe$\"' 0 grok_complete_done
  ReadRegStr $0 SHCTX "${MANUPRODUCTKEY}" ""
  StrCmp $0 "$INSTDIR" 0 grok_complete_done
  IfFileExists "$INSTDIR\${MAINBINARYNAME}.exe" 0 grok_complete_done
  IfFileExists "$INSTDIR\uninstall.exe" 0 grok_complete_done

  !insertmacro GROK_WRITE_UPDATE_COMPLETION_RECEIPT grok_complete_done
  grok_complete_done:
  Pop $9
  Pop $8
  Pop $7
  Pop $6
  Pop $5
  Pop $4
  Pop $3
  Pop $2
  Pop $1
  Pop $0
FunctionEnd
!macroend

; Failure callbacks provide evidence only, never rollback or replay permission.
!macro GROK_DEFINE_UPDATE_FAILURE
Var GrokUpdateFailure
Var GrokUpdateFailureReason
Function GrokReportUpdateFailed
  StrCpy $GrokUpdateFailureReason "failed"
  Call GrokWriteUpdateFailure
FunctionEnd
Function GrokReportUpdateCancelled
  StrCpy $GrokUpdateFailureReason "cancelled"
  Call GrokWriteUpdateFailure
FunctionEnd
Function GrokWriteUpdateFailure
  Push $0
  Push $1
  Push $2
  Push $3
  Push $4
  Push $5
  Push $6
  ${If} $UpdateMode != 1
    Goto grok_failure_done
  ${EndIf}
  !if "${INSTALLMODE}" != "currentUser"
    Goto grok_failure_done
  !endif
  ClearErrors
  ${GetOptions} $CMDLINE "/GROKUPDATEID=" $GrokUpdateId
  IfErrors grok_failure_done
  ${GetOptions} $CMDLINE "/GROKUPDATEFAILURE=" $GrokUpdateFailure
  IfErrors grok_failure_done
  StrCmp $GrokUpdateFailure "" grok_failure_done
  !insertmacro GROK_VALIDATE_UPDATE_NONCE grok_failure grok_failure_done
  ; The target registration/files can be partial here: do not call the success
  ; gate or claim that they were restored. Rust still requires exact process
  ; exit evidence and a publisher-declared failure protocol.
  System::Call 'kernel32::GetCurrentProcess() p.r1'
  System::Call 'kernel32::GetProcessTimes(p r1, *l .r2, *l .r3, *l .r4, *l .r5) i.r6'
  StrCmp $6 0 grok_failure_done
  System::Call 'kernel32::GetCurrentProcessId() i.r3'
  System::Call 'kernel32::CreateFileW(w "$GrokUpdateFailure", i 0x40000000, i 1, p 0, i 1, i 0x80200080, p 0) p.r0'
  StrCmp $0 -1 grok_failure_done
  ClearErrors
  FileWriteUTF16LE /BOM $0 "grok-nsis-install-failed-v1$\r$\n$GrokUpdateId$\r$\n${VERSION}$\r$\n${PRODUCTNAME}$\r$\n${MAINBINARYNAME}.exe$\r$\n${BUNDLEID}$\r$\n$INSTDIR$\r$\n$3$\r$\n$2$\r$\n$GrokUpdateFailureReason$\r$\n"
  System::Call 'kernel32::FlushFileBuffers(p r0) i.r1'
  System::Call 'kernel32::CloseHandle(p r0)'
  grok_failure_done:
  Pop $6
  Pop $5
  Pop $4
  Pop $3
  Pop $2
  Pop $1
  Pop $0
FunctionEnd
!macroend
