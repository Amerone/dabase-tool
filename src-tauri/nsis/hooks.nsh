; Tauri NSIS installer hooks for packaged database drivers.
; installMode = perMachine, so HKLM ODBC driver registration is available.

!include LogicLib.nsh

!macro REGISTER_ODBC_DRIVER DRIVER_NAME DRIVER_DLL
  ${If} ${FileExists} "${DRIVER_DLL}"
    WriteRegStr HKLM "SOFTWARE\ODBC\ODBCINST.INI\ODBC Drivers" \
        "${DRIVER_NAME}" "Installed"
    WriteRegStr HKLM "SOFTWARE\ODBC\ODBCINST.INI\${DRIVER_NAME}" \
        "Driver" "${DRIVER_DLL}"
    WriteRegStr HKLM "SOFTWARE\ODBC\ODBCINST.INI\${DRIVER_NAME}" \
        "Setup" "${DRIVER_DLL}"
    WriteRegStr HKLM "SOFTWARE\ODBC\ODBCINST.INI\${DRIVER_NAME}" \
        "APILevel" "1"
    WriteRegStr HKLM "SOFTWARE\ODBC\ODBCINST.INI\${DRIVER_NAME}" \
        "DriverODBCVer" "03.00"
    WriteRegStr HKLM "SOFTWARE\ODBC\ODBCINST.INI\${DRIVER_NAME}" \
        "FileUsage" "0"
  ${EndIf}
!macroend

!macro UNREGISTER_ODBC_DRIVER DRIVER_NAME
  DeleteRegValue HKLM "SOFTWARE\ODBC\ODBCINST.INI\ODBC Drivers" \
      "${DRIVER_NAME}"
  DeleteRegKey HKLM "SOFTWARE\ODBC\ODBCINST.INI\${DRIVER_NAME}"
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro REGISTER_ODBC_DRIVER \
      "Amarone DM8 ODBC Driver" \
      "$INSTDIR\drivers\dm8\windows\dodbc.dll"

  !insertmacro REGISTER_ODBC_DRIVER \
      "Amarone KingbaseES 9 ODBC Driver ANSI" \
      "$INSTDIR\drivers\kingbase\X64_Windows\odbc\x64_ANSI_Release\kdbodbc30a.dll"

  !insertmacro REGISTER_ODBC_DRIVER \
      "Amarone PostgreSQL Unicode" \
      "$INSTDIR\drivers\postgresql\windows\psqlodbc35w.dll"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  !insertmacro UNREGISTER_ODBC_DRIVER "Amarone DM8 ODBC Driver"
  !insertmacro UNREGISTER_ODBC_DRIVER "Amarone KingbaseES 9 ODBC Driver ANSI"
  !insertmacro UNREGISTER_ODBC_DRIVER "Amarone PostgreSQL Unicode"
!macroend
