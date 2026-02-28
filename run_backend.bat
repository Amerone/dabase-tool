@echo off
call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvarsall.bat" x64
if errorlevel 1 (
    echo Failed to set up Visual Studio environment
    exit /b 1
)
set DM8_DRIVER_PATH=E:\self\tool-database\drivers\dm8\windows\dodbc.dll
set PATH=E:\self\tool-database\drivers\dm8\windows;%PATH%
cd /d E:\self\tool-database\backend
cargo run
