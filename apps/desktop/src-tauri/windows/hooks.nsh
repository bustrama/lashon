; Ottid NSIS installer hooks (docs/adr/0042).
;
; Until v1.1 the product shipped as "Lashon", registered under the uninstall
; key "...\Uninstall\Lashon". This installer (product "Ottid") does not see
; that as a previous version of itself, so without this hook a machine that
; had Lashon would end up with both apps.
;
; The hook runs the old uninstaller silently. A silent uninstall never deletes
; app data (its "delete app data" box stays unticked), so settings, history
; and models stay where they are; Ottid moves them to its own identifier on
; first launch (ottid_core::legacy).

!macro OTTID_REMOVE_LASHON ROOT VERB
  ReadRegStr $R0 ${ROOT} "Software\Microsoft\Windows\CurrentVersion\Uninstall\Lashon" "InstallLocation"
  ${If} $R0 != ""
    ; Tauri stores the location quoted: "C:\Program Files\Lashon".
    StrCpy $R1 $R0 1
    ${If} $R1 == '"'
      StrCpy $R0 $R0 -1 1
    ${EndIf}
    ${If} ${FileExists} "$R0\uninstall.exe"
      DetailPrint "Removing the previous version (Lashon) from $R0"
      ; `_?=` runs the uninstaller in place, so the wait really waits for it.
      ; A per-machine install needs admin rights to remove: "runas" asks for
      ; them (no prompt when this installer is already elevated).
      ExecShellWait "${VERB}" "$R0\uninstall.exe" "/S _?=$R0"
      ; An in-place uninstaller can't delete itself.
      Delete "$R0\uninstall.exe"
      RMDir "$R0"
    ${EndIf}
    ; Installer bookkeeping the silent uninstall leaves behind.
    DeleteRegKey ${ROOT} "Software\lashon\Lashon"
    DeleteRegKey /ifempty ${ROOT} "Software\lashon"
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  Push $R0
  Push $R1
  !insertmacro OTTID_REMOVE_LASHON HKCU "open"
  !insertmacro OTTID_REMOVE_LASHON HKLM "runas"
  Pop $R1
  Pop $R0
!macroend
