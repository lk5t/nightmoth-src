; Nightmoth installer hooks.
;
; The taskbar helper (nightmoth_tap.dll) lives inside Windows Explorer while the
; invisible taskbar is on, so Windows won't let the installer overwrite it. A file
; that's in use CAN be renamed, though: move it aside under a unique name, then
; have Windows delete it at the next restart. The new helper installs cleanly.

!macro NightmothMoveHelperAside
  IfFileExists "$INSTDIR\nightmoth_tap.dll" 0 +4
    System::Call 'kernel32::GetTickCount()i .r9'
    Rename "$INSTDIR\nightmoth_tap.dll" "$INSTDIR\nightmoth_tap.$9.old"
    Delete /REBOOTOK "$INSTDIR\nightmoth_tap.$9.old"
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro NightmothMoveHelperAside
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro NightmothMoveHelperAside
!macroend
