@echo off
setlocal EnableExtensions

rem One-click Windows package script for DM8 Export Tool.
rem Output installer:
rem   src-tauri\target\release\bundle\nsis\DM8 Export Tool_0.1.0_x64-setup.exe

chcp 65001 >nul

set "ROOT=%~dp0"
set "ROOT=%ROOT:~0,-1%"
set "PS=%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe"
set "BUILD_PS=%ROOT%\build_windows.ps1"
set "INSTALLER=%ROOT%\src-tauri\target\release\bundle\nsis\DM8 Export Tool_0.1.0_x64-setup.exe"
set "APP_EXE=%ROOT%\src-tauri\target\release\dm8-export-tauri.exe"

rem Tauri CLI expects CI to be "true" or "false" when it is set.
if /i "%CI%"=="1" set "CI=true"
if /i "%CI%"=="0" set "CI=false"

echo ============================================================
echo DM8 Export Tool - Windows EXE packaging
echo Project root: %ROOT%
echo ============================================================
echo.

if not exist "%PS%" (
  echo ERROR: PowerShell was not found at:
  echo   %PS%
  exit /b 1
)

if not exist "%BUILD_PS%" (
  echo ERROR: build script not found:
  echo   %BUILD_PS%
  exit /b 1
)

echo [1/3] Stopping running project dev processes...
"%PS%" -NoProfile -ExecutionPolicy Bypass -Command "$root=(Resolve-Path '%ROOT%').Path; Get-CimInstance Win32_Process | Where-Object { $_.Name -in @('dm8-export-backend.exe','node.exe','cargo.exe') -and (($_.ExecutablePath -like ($root + '\*')) -or ($_.CommandLine -like ('*' + $root + '*'))) } | ForEach-Object { Write-Host ('Stopping PID ' + $_.ProcessId + ' ' + $_.Name); Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }"
if errorlevel 1 (
  echo ERROR: Failed while stopping project processes.
  exit /b 1
)

echo.
echo [2/3] Running Tauri/NSIS build...
"%PS%" -NoProfile -ExecutionPolicy Bypass -File "%BUILD_PS%"
if errorlevel 1 (
  echo.
  echo ERROR: Packaging failed.
  exit /b 1
)

echo.
echo [3/3] Build output
if exist "%INSTALLER%" (
  echo Installer:
  echo   %INSTALLER%
  echo.
  echo SHA256:
  "%PS%" -NoProfile -ExecutionPolicy Bypass -Command "(Get-FileHash -LiteralPath '%INSTALLER%' -Algorithm SHA256).Hash"
) else (
  echo WARNING: Installer was not found at expected path:
  echo   %INSTALLER%
)

if exist "%APP_EXE%" (
  echo.
  echo Release app exe:
  echo   %APP_EXE%
)

echo.
echo Done.
if not "%NO_PAUSE%"=="1" pause
