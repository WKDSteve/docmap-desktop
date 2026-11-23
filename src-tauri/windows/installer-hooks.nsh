; Hooks of the NSIS setup of docmap-desktop (tauri.windows.conf.json, bundle.windows.nsis.installerHooks).
;
; The setup makes the command `docmap-desktop` for every new terminal: a starter in the family folder
; (%LOCALAPPDATA%\appkit-family\bin) that is put on the PATH of the user once, as every app of the family does with its
; own install (appkit docs/FAMILY.md, "The command in the terminal"). The work is the program's own
; (`docmap-desktop.exe --install-command`, src/command_entry.rs, tested there): this script only calls it, so the starter
; has the same text whoever writes it and the PATH of the user is changed by one piece of code.
;
; The setup never fails because of it: a command that could not be made is the one thing that is missing, and
; `docmap-desktop --install-command` (or `hub-desktop cli repair`) makes it later. The windows of the setup stay as
; they are: the program writes nothing to a window.

!macro NSIS_HOOK_POSTINSTALL
  Push $0
  ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --install-command' $0
  Pop $0
!macroend

; Before the files go: the starter names the program, and the folder and its PATH entry go with the last starter.
!macro NSIS_HOOK_PREUNINSTALL
  Push $0
  ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --uninstall-command' $0
  Pop $0
!macroend
