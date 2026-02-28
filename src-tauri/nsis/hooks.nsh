; hooks.nsh — Tauri NSIS installer hooks for DM8 Export Tool
; 文件由 Tauri 在 NSIS 安装脚本中自动引用（bundle.nsis.installerHooks）
; 安装器以管理员身份运行（installMode = perMachine），可安全写入 HKLM

; 安装完成后注册 DM8 ODBC 驱动
; 此时文件已复制到 $INSTDIR，可安全引用驱动 DLL 路径
!macro NSIS_HOOK_POSTINSTALL
  WriteRegStr HKLM "SOFTWARE\ODBC\ODBCINST.INI\ODBC Drivers" \
      "DM8 ODBC Driver" "Installed"

  WriteRegStr HKLM "SOFTWARE\ODBC\ODBCINST.INI\DM8 ODBC Driver" \
      "Driver" "$INSTDIR\drivers\dm8\windows\dodbc.dll"
  WriteRegStr HKLM "SOFTWARE\ODBC\ODBCINST.INI\DM8 ODBC Driver" \
      "Setup" "$INSTDIR\drivers\dm8\windows\dodbc.dll"
  WriteRegStr HKLM "SOFTWARE\ODBC\ODBCINST.INI\DM8 ODBC Driver" \
      "APILevel" "1"
  WriteRegStr HKLM "SOFTWARE\ODBC\ODBCINST.INI\DM8 ODBC Driver" \
      "DriverODBCVer" "03.00"
  WriteRegStr HKLM "SOFTWARE\ODBC\ODBCINST.INI\DM8 ODBC Driver" \
      "FileUsage" "0"
!macroend

; 卸载时删除 ODBC 驱动注册信息
!macro NSIS_HOOK_POSTUNINSTALL
  DeleteRegValue HKLM "SOFTWARE\ODBC\ODBCINST.INI\ODBC Drivers" \
      "DM8 ODBC Driver"
  DeleteRegKey   HKLM "SOFTWARE\ODBC\ODBCINST.INI\DM8 ODBC Driver"
!macroend
