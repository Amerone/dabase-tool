@echo off
call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvarsall.bat" x64
if errorlevel 1 (
    echo Failed to set up Visual Studio environment
    exit /b 1
)
set "REPO_ROOT=%~dp0"
set "DM8_DRIVER_PATH=%REPO_ROOT%drivers\dm8\windows\dodbc.dll"
set "PATH=%REPO_ROOT%drivers\dm8\windows;%PATH%"
set "DM8_ODBC_DRIVER="
cd /d "%REPO_ROOT%backend"
cargo run
