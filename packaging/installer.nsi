; PoE2 Oracle installer (NSIS 3, built by packaging/build-release.ps1 with the official NSIS 3.12
; zip). Per-user, no admin rights: installs to %LOCALAPPDATA%\Programs\PoE2 Oracle, lists itself in
; Windows' "Installed apps", adds a Start menu shortcut and, when asked, the same autostart entry the
; app's settings write.
;
;   makensis /INPUTCHARSET UTF8 /DVERSION=0.1.0 packaging\installer.nsi
;
; Optional defines, paths relative to this file: APP_EXE_PATH (default
; ..\target\release\poe2-oracle.exe) and OUT_DIR (default ..\target\dist, must exist and hold the
; THIRD-PARTY-NOTICES.html build-release.ps1 writes there first). The output is
; ${OUT_DIR}\PoE2-Oracle-Setup-${VERSION}.exe -- the asset name crates/auto-update looks for.
;
; Command line:
;   /S          silent (NSIS built-in): no UI, the autostart entry is left exactly as it was
;   /relaunch   with /S: start PoE2 Oracle once done -- the new copy, or the old one if installing
;               failed. crates/auto-update starts updates as `/S /relaunch`, then quits the app.
;   /D=<dir>    install directory (NSIS built-in, must come last)
;
; The uninstaller removes the program with its license files, the shortcut, the autostart entry and
; the "Installed apps" entry; settings and caches only when the user ticks "Settings and cache"
; (never in /S mode).

Unicode true
ManifestDPIAware true
RequestExecutionLevel user
SetCompressor /SOLID lzma
; A file still locked by a lingering process must fail the install (and, for /relaunch, restart the
; old copy) instead of being skipped: skipping would report success with the old exe in place.
AllowSkipFiles off

!ifndef VERSION
  !error "Pass the app version: makensis /DVERSION=x.y.z installer.nsi"
!endif
!define /ifndef APP_EXE_PATH "..\target\release\poe2-oracle.exe"
!define /ifndef OUT_DIR "..\target\dist"

!define PRODUCT_NAME "PoE2 Oracle"
!define PUBLISHER "mttzzz"
!define APP_EXE "poe2-oracle.exe"
!define UNINSTALLER "uninstall.exe"
; Beside the exe: the app's two license texts (the repository's LICENSE-MIT and LICENSE-APACHE,
; named .txt so a double-click opens them) and the third-party notices.
!define LICENSE_MIT "LICENSE-MIT.txt"
!define LICENSE_APACHE "LICENSE-APACHE.txt"
!define NOTICES "THIRD-PARTY-NOTICES.html"
!define ICON "..\crates\poe2-oracle\assets\icon\poe2-oracle.ico"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT_NAME}"
; The autostart entry, exactly as the app's settings write it (agreed with the settings module
; 2026-09-22): HKCU Run value "PoE2 Oracle" = "<exe>" --autostart. Task Manager's "Disable" does
; not touch Run but writes a same-named value under StartupApproved\Run; enabling clears that value,
; as the app does, so that enabling here actually takes effect.
!define RUN_KEY "Software\Microsoft\Windows\CurrentVersion\Run"
!define STARTUP_APPROVED_KEY "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"
!define RUN_VALUE "${PRODUCT_NAME}"
; The running app's hidden door window (crates/poe2-oracle/src/platform/instance.rs): WM_CLOSE to
; it quits the app the normal way.
!define DOOR_CLASS "PoE2Oracle.Instance"
; directories::ProjectDirs::from("", "", "poe2-oracle"): settings under %APPDATA%\poe2-oracle,
; caches (prices, catalogs, downloaded updates) under %LOCALAPPDATA%\poe2-oracle.
!define DATA_DIR "poe2-oracle"

; VIProductVersion takes four numbers: cut a pre-release suffix ("0.2.0-rc.1" -> "0.2.0").
!searchparse "${VERSION}-" "" VERSION_CORE "-"

Name "${PRODUCT_NAME}"
OutFile "${OUT_DIR}\PoE2-Oracle-Setup-${VERSION}.exe"
InstallDir "$LOCALAPPDATA\Programs\${PRODUCT_NAME}"
; Reinstalls and updates go wherever the previous install went.
InstallDirRegKey HKCU "${UNINSTALL_KEY}" "InstallLocation"
BrandingText "${PRODUCT_NAME} ${VERSION}"

VIProductVersion "${VERSION_CORE}.0"
VIFileVersion "${VERSION_CORE}.0"
VIAddVersionKey /LANG=0 "ProductName" "${PRODUCT_NAME}"
VIAddVersionKey /LANG=0 "ProductVersion" "${VERSION}"
VIAddVersionKey /LANG=0 "FileVersion" "${VERSION}"
VIAddVersionKey /LANG=0 "FileDescription" "${PRODUCT_NAME} Setup"
VIAddVersionKey /LANG=0 "CompanyName" "${PUBLISHER}"
VIAddVersionKey /LANG=0 "LegalCopyright" "Copyright (c) 2026 ${PUBLISHER}"

!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"
!include "WinMessages.nsh"

!define MUI_ICON "${ICON}"
!define MUI_UNICON "${ICON}"
!define MUI_ABORTWARNING

!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
; The Finish page's second checkbox is the autostart choice: preset from the registry when the page
; shows, applied when the user clicks Finish. A silent run never reaches this page, which is what
; keeps /S updates from changing autostart.
!define MUI_FINISHPAGE_NOREBOOTSUPPORT
!define MUI_FINISHPAGE_RUN "$INSTDIR\${APP_EXE}"
!define MUI_FINISHPAGE_SHOWREADME
!define MUI_FINISHPAGE_SHOWREADME_TEXT "$(AutostartOption)"
!define MUI_FINISHPAGE_SHOWREADME_NOTCHECKED
!define MUI_FINISHPAGE_SHOWREADME_FUNCTION EnableAutostart
!define MUI_PAGE_CUSTOMFUNCTION_SHOW FinishPageShow
!define MUI_PAGE_CUSTOMFUNCTION_LEAVE FinishPageLeave
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_COMPONENTS
!insertmacro MUI_UNPAGE_INSTFILES

; The first language is the fallback for any Windows display language other than these two.
!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "Russian"

LangString AutostartOption ${LANG_ENGLISH} "Start with Windows"
LangString AutostartOption ${LANG_RUSSIAN} "Запускать вместе с Windows"
LangString AppRunning ${LANG_ENGLISH} "PoE2 Oracle is running and will be closed to continue."
LangString AppRunning ${LANG_RUSSIAN} "PoE2 Oracle запущен и будет закрыт, чтобы продолжить."
LangString AppNotClosed ${LANG_ENGLISH} "PoE2 Oracle could not be closed. Quit it from its tray icon, then click Retry."
LangString AppNotClosed ${LANG_RUSSIAN} "Не удалось закрыть PoE2 Oracle. Закройте его через значок в трее и нажмите «Повторить»."
LangString ClosingApp ${LANG_ENGLISH} "Closing PoE2 Oracle..."
LangString ClosingApp ${LANG_RUSSIAN} "Закрываю PoE2 Oracle..."
LangString ProgramDescription ${LANG_ENGLISH} "The program, its Start menu shortcut and its autostart entry."
LangString ProgramDescription ${LANG_RUSSIAN} "Программа, её ярлык в меню «Пуск» и автозапуск."
LangString SettingsSection ${LANG_ENGLISH} "Settings and cache"
LangString SettingsSection ${LANG_RUSSIAN} "Настройки и кэш"
LangString SettingsDescription ${LANG_ENGLISH} "Also delete your settings and the downloaded price data. Leave unticked to keep them for a reinstall."
LangString SettingsDescription ${LANG_RUSSIAN} "Удалить также настройки и загруженные данные о ценах. Оставьте без отметки, чтобы сохранить их для переустановки."

; Sets _RESULT to 0 when this user runs a PoE2 Oracle process: `find` exits 0 when tasklist listed
; one. Every tool by full path, since a Unix `find` earlier on PATH (Git's usr\bin) would answer
; differently; the outer quotes are the ones cmd /c strips. electron-builder's per-user NSIS
; template detects running apps the same way.
!macro FIND_RUNNING_APP _RESULT
  nsExec::Exec `"$SYSDIR\cmd.exe" /c ""$SYSDIR\tasklist.exe" /FI "USERNAME eq %USERNAME%" /FI "IMAGENAME eq ${APP_EXE}" /FO CSV /NH | "$SYSDIR\find.exe" /I "${APP_EXE}""`
  Pop ${_RESULT}
!macroend

; Closes this user's running PoE2 Oracle, which keeps its exe locked -- an interactive run asks
; first. Politely first: WM_CLOSE to the app's door window (crates/poe2-oracle/src/platform/
; instance.rs) quits it the normal way, tray icon removed, and an updating app is quitting on its
; own already (crates/auto-update starts the installer, then the app exits); either gets 10 s.
; Then force-closing: gpui leaves its message loop on no window message (gpui_windows events.rs
; at the pinned rev), so a copy without the door -- or one that doesn't go -- is killed, which
; leaves its tray icon behind until the mouse passes over it.
!macro CLOSE_RUNNING_APP_FUNCTION PREFIX
  Function ${PREFIX}CloseRunningApp
    Push $R0
    Push $R1
    !insertmacro FIND_RUNNING_APP $R0
    ${If} $R0 == 0
    ${AndIfNot} ${Silent}
      MessageBox MB_OKCANCEL|MB_ICONINFORMATION "$(AppRunning)" IDOK close_app
      Abort
    ${EndIf}
    close_app:
    ${If} $R0 == 0
      DetailPrint "$(ClosingApp)"
      FindWindow $R1 "${DOOR_CLASS}"
      ${If} $R1 <> 0
        SendMessage $R1 ${WM_CLOSE} 0 0 /TIMEOUT=2000
      ${EndIf}
      ${If} $R1 <> 0
      ${OrIf} ${Silent}
        StrCpy $R1 0
        ${Do}
          Sleep 250
          !insertmacro FIND_RUNNING_APP $R0
          ${If} $R0 != 0
            ${ExitDo}
          ${EndIf}
          IntOp $R1 $R1 + 1
        ${LoopUntil} $R1 >= 40
      ${EndIf}
    ${EndIf}
    ${If} $R0 == 0
      StrCpy $R1 0
      ${Do}
        nsExec::Exec `"$SYSDIR\cmd.exe" /c ""$SYSDIR\taskkill.exe" /F /FI "USERNAME eq %USERNAME%" /IM "${APP_EXE}""`
        Pop $R0
        Sleep 500
        !insertmacro FIND_RUNNING_APP $R0
        ${If} $R0 != 0
          ${ExitDo}
        ${EndIf}
        IntOp $R1 $R1 + 1
        ${If} $R1 >= 6
          MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "$(AppNotClosed)" /SD IDCANCEL IDRETRY retry_close
          Abort
          retry_close:
          StrCpy $R1 0
        ${EndIf}
      ${Loop}
    ${EndIf}
    ; A process that just left the process list can hold its exe a moment longer.
    ${If} ${FileExists} "$INSTDIR\${APP_EXE}"
      StrCpy $R1 0
      ${Do}
        ClearErrors
        FileOpen $R0 "$INSTDIR\${APP_EXE}" a
        ${IfNot} ${Errors}
          FileClose $R0
          ${ExitDo}
        ${EndIf}
        Sleep 250
        IntOp $R1 $R1 + 1
      ${LoopUntil} $R1 >= 40
    ${EndIf}
    Pop $R1
    Pop $R0
  FunctionEnd
!macroend
!insertmacro CLOSE_RUNNING_APP_FUNCTION ""
!insertmacro CLOSE_RUNNING_APP_FUNCTION "un."

Var Relaunch

Function .onInit
  ${GetParameters} $R0
  ClearErrors
  ${GetOptions} $R0 "/relaunch" $R1
  ${IfNot} ${Errors}
  ${AndIf} ${Silent}
    StrCpy $Relaunch 1
  ${EndIf}
FunctionEnd

Section "-${PRODUCT_NAME}"
  SetOutPath "$INSTDIR"
  Call CloseRunningApp
  File "/oname=${APP_EXE}" "${APP_EXE_PATH}"
  File "/oname=${LICENSE_MIT}" "..\LICENSE-MIT"
  File "/oname=${LICENSE_APACHE}" "..\LICENSE-APACHE"
  File "${OUT_DIR}\${NOTICES}"
  WriteUninstaller "$INSTDIR\${UNINSTALLER}"
  CreateShortcut "$SMPROGRAMS\${PRODUCT_NAME}.lnk" "$INSTDIR\${APP_EXE}"

  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${PRODUCT_NAME}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\${APP_EXE},0"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "${PUBLISHER}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "URLInfoAbout" "https://github.com/mttzzz/poe2-oracle"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\${UNINSTALLER}"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\${UNINSTALLER}" /S'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $R0 $R1 $R2
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "EstimatedSize" $R0
SectionEnd

Function .onInstSuccess
  ${If} $Relaunch == 1
    Exec '"$INSTDIR\${APP_EXE}"'
  ${EndIf}
FunctionEnd

Function .onInstFailed
  ; Files are replaced only after the old copy closed, so whatever exe is in place still runs:
  ; an update that failed must not leave the player without the app -- unless the old copy never
  ; closed (the reason it failed), in which case the player still has it.
  ${If} $Relaunch == 1
  ${AndIf} ${FileExists} "$INSTDIR\${APP_EXE}"
    !insertmacro FIND_RUNNING_APP $R0
    ${If} $R0 != 0
      Exec '"$INSTDIR\${APP_EXE}"'
    ${EndIf}
  ${EndIf}
FunctionEnd

Function FinishPageShow
  ReadRegStr $R0 HKCU "${RUN_KEY}" "${RUN_VALUE}"
  ${If} $R0 != ""
    ${NSD_Check} $mui.FinishPage.ShowReadme
  ${EndIf}
FunctionEnd

; Runs before the Finish page's own actions: an unticked box turns autostart off, both values, as
; the app's settings do.
Function FinishPageLeave
  ${NSD_GetState} $mui.FinishPage.ShowReadme $R0
  ${If} $R0 != ${BST_CHECKED}
    DeleteRegValue HKCU "${RUN_KEY}" "${RUN_VALUE}"
    DeleteRegValue HKCU "${STARTUP_APPROVED_KEY}" "${RUN_VALUE}"
  ${EndIf}
FunctionEnd

Function EnableAutostart
  WriteRegStr HKCU "${RUN_KEY}" "${RUN_VALUE}" '"$INSTDIR\${APP_EXE}" --autostart'
  DeleteRegValue HKCU "${STARTUP_APPROVED_KEY}" "${RUN_VALUE}"
FunctionEnd

Section "un.${PRODUCT_NAME}" UninstallProgram
  SectionIn RO
  Call un.CloseRunningApp
  Delete "$INSTDIR\${APP_EXE}"
  Delete "$INSTDIR\${LICENSE_MIT}"
  Delete "$INSTDIR\${LICENSE_APACHE}"
  Delete "$INSTDIR\${NOTICES}"
  Delete "$INSTDIR\${UNINSTALLER}"
  ; Not /r: the user may have chosen a folder that holds other files.
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\${PRODUCT_NAME}.lnk"
  DeleteRegValue HKCU "${RUN_KEY}" "${RUN_VALUE}"
  DeleteRegValue HKCU "${STARTUP_APPROVED_KEY}" "${RUN_VALUE}"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
SectionEnd

Section /o "un.$(SettingsSection)" UninstallSettings
  RMDir /r "$APPDATA\${DATA_DIR}"
  RMDir /r "$LOCALAPPDATA\${DATA_DIR}"
SectionEnd

!insertmacro MUI_UNFUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${UninstallProgram} "$(ProgramDescription)"
  !insertmacro MUI_DESCRIPTION_TEXT ${UninstallSettings} "$(SettingsDescription)"
!insertmacro MUI_UNFUNCTION_DESCRIPTION_END
