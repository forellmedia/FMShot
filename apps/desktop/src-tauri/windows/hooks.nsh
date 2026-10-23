; Ergänzungen zum NSIS-Installer von Tauri (bundle.windows.nsis.installerHooks).
;
; Nabshot installiert für alle Benutzer nach „Programme\Nabshot“. Frühere
; Versionen lagen pro Benutzer unter AppData (auch unter den alten Namen
; Grabmark und FMShot, ADR 0013/0015). Die werden vorher still entfernt –
; Bibliothek und Einstellungen bleiben, das Deinstallieren löscht sie nur auf
; ausdrücklichen Wunsch. „Mit Anmeldung starten“ wird übernommen.

Var NabOldUninst
Var NabOldDir
Var NabAutostart
; Eigene Variablen statt $0/$1: Der Tauri-Installer braucht seine weiter.
Var NabTmp
Var NabTmp2

!macro NAB_REMOVE_USER_INSTALL NAME
  ReadRegStr $NabOldUninst HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${NAME}" "UninstallString"
  ReadRegStr $NabOldDir HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${NAME}" "InstallLocation"
  ${If} $NabOldUninst != ""
  ${AndIf} $NabOldDir != ""
    ; InstallLocation steht in Anführungszeichen.
    StrCpy $NabTmp $NabOldDir 1
    ${If} $NabTmp == '"'
      StrCpy $NabOldDir $NabOldDir -1 1
    ${EndIf}
    DetailPrint "Frühere Installation entfernen: $NabOldDir"
    ; _?= lässt das Deinstallationsprogramm an Ort und Stelle laufen und
    ; wartet darauf; danach liegen nur noch es selbst und der Ordner da.
    ExecWait '$NabOldUninst /S _?=$NabOldDir'
    Delete "$NabOldDir\uninstall.exe"
    RMDir "$NabOldDir"
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  ; Vor dem Entfernen merken: Das alte Deinstallationsprogramm löscht den
  ; Eintrag unter „Run“.
  StrCpy $NabAutostart "0"
  ReadRegStr $NabTmp HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Nabshot"
  ReadRegStr $NabTmp2 HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Grabmark"
  ${If} $NabTmp != ""
  ${OrIf} $NabTmp2 != ""
    StrCpy $NabAutostart "1"
  ${EndIf}
  !insertmacro NAB_REMOVE_USER_INSTALL "Nabshot"
  !insertmacro NAB_REMOVE_USER_INSTALL "Grabmark"
  !insertmacro NAB_REMOVE_USER_INSTALL "FMShot"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Grabmark"
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ${If} $NabAutostart == "1"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${PRODUCTNAME}" '"$INSTDIR\${MAINBINARYNAME}.exe"'
  ${EndIf}
!macroend
