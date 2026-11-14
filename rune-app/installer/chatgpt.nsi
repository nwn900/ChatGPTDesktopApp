; ChatGPT installer.
; Build: makensis chatgpt.nsi   (from rune-app/installer)
; Installs per-user into %LOCALAPPDATA%, so no elevation is needed.

Unicode true
!include "MUI2.nsh"
!include "LogicLib.nsh"

!define PRODUCT "ChatGPT"
!define VERSION "1.0.16"
!define EXE "chatgpt.exe"
!define PUBLISHER "nwn900"
; Namespaced so it cannot collide with another ChatGPT program's entry.
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\{7B3E1F2A-9C4D-4E8B-A1F6-5D2C8E4B9A70}"
; The Run value the app's own startup toggle writes.
!define RUN_KEY "Software\Microsoft\Windows\CurrentVersion\Run"
!define RUN_VALUE "ChatGPT"
; The Evergreen WebView2 Runtime.
!define WEBVIEW2_GUID "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"

Name "${PRODUCT} ${VERSION}"
OutFile "..\dist\ChatGPT_${VERSION}_x64-setup.exe"
InstallDir "$LOCALAPPDATA\ChatGPT"
RequestExecutionLevel user
SetCompressor /SOLID lzma

!define MUI_ICON "..\..\src-tauri\icons\icon.ico"
!define MUI_UNICON "..\..\src-tauri\icons\icon.ico"
!define MUI_FINISHPAGE_RUN "$INSTDIR\${EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Run ${PRODUCT}"

!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

; Ask a running copy to quit so its executable is not locked mid-upgrade.
Function QuitRunningApp
  ${If} ${FileExists} "$INSTDIR\${EXE}"
    System::Call 'user32::FindWindowW(w "Window Class", w "${PRODUCT}") p .r1'
    ${If} $1 != 0
      DetailPrint "Asking the running app to quit"
      ExecWait '"$INSTDIR\${EXE}" --quit'
      Sleep 1200
    ${EndIf}
  ${EndIf}
FunctionEnd

; Evergreen Runtime present? Check both machine and user installs.
Function CheckWebView2
  StrCpy $0 ""
  ReadRegStr $0 HKLM "SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\${WEBVIEW2_GUID}" "pv"
  ${If} $0 == ""
    ReadRegStr $0 HKCU "Software\Microsoft\EdgeUpdate\Clients\${WEBVIEW2_GUID}" "pv"
  ${EndIf}
  ${If} $0 == ""
    MessageBox MB_ICONEXCLAMATION|MB_OK "Microsoft Edge WebView2 Runtime was not found.$\r$\n$\r$\nChatGPT needs it to show the page. Install it from https://developer.microsoft.com/microsoft-edge/webview2/ and start ChatGPT again."
  ${EndIf}
FunctionEnd

Section "Install"
  SetShellVarContext current
  Call QuitRunningApp
  SetOutPath "$INSTDIR"
  File "..\target\release\${EXE}"
  WriteUninstaller "$INSTDIR\uninstall.exe"
  Call CheckWebView2

  CreateShortcut "$DESKTOP\${PRODUCT}.lnk" "$INSTDIR\${EXE}"
  CreateDirectory "$SMPROGRAMS\${PRODUCT}"
  CreateShortcut "$SMPROGRAMS\${PRODUCT}\${PRODUCT}.lnk" "$INSTDIR\${EXE}"
  CreateShortcut "$SMPROGRAMS\${PRODUCT}\Uninstall ${PRODUCT}.lnk" "$INSTDIR\uninstall.exe"

  WriteRegStr HKCU "${UNINST_KEY}" "DisplayName" "${PRODUCT}"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINST_KEY}" "Publisher" "${PUBLISHER}"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\${EXE}"
  WriteRegStr HKCU "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
  ; Quoted: an unquoted path with spaces parses as a command plus arguments.
  WriteRegStr HKCU "${UNINST_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "${UNINST_KEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoRepair" 1

  ; Migrate the pre-1.0.15 entry, which used a plain "ChatGPT" key.
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ChatGPT"
  DeleteRegKey HKCU "Software\ChatGPT"
SectionEnd

Section "Uninstall"
  SetShellVarContext current
  ; Only this app's own value, verified by name.
  DeleteRegValue HKCU "${RUN_KEY}" "${RUN_VALUE}"

  Delete "$INSTDIR\${EXE}"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"

  Delete "$DESKTOP\${PRODUCT}.lnk"
  Delete "$SMPROGRAMS\${PRODUCT}\${PRODUCT}.lnk"
  Delete "$SMPROGRAMS\${PRODUCT}\Uninstall ${PRODUCT}.lnk"
  RMDir "$SMPROGRAMS\${PRODUCT}"

  DeleteRegKey HKCU "${UNINST_KEY}"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ChatGPT"
  DeleteRegKey HKCU "Software\ChatGPT"
  ; The WebView2 profile is deliberately left in place: it holds the sign-in.
SectionEnd
