; Installer hooks for the Windows (NSIS) build.
;
; `lume` works from any terminal after installing: a small launcher is placed in
; %LOCALAPPDATA%\Microsoft\WindowsApps, a folder Windows already keeps on every user's PATH, so no
; PATH editing is needed and removing it on uninstall is a single file delete.

!macro NSIS_HOOK_POSTINSTALL
  ClearErrors
  FileOpen $0 "$LOCALAPPDATA\Microsoft\WindowsApps\lume.cmd" w
  IfErrors +4
    FileWrite $0 "@echo off$\r$\n"
    FileWrite $0 'start "" /b /wait "$INSTDIR\${MAINBINARYNAME}.exe" %*$\r$\n'
    FileClose $0
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  Delete "$LOCALAPPDATA\Microsoft\WindowsApps\lume.cmd"
!macroend
