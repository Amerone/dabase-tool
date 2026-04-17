@echo off
if "%SHENTONG_SMB_HOST%"=="" (
  echo Set SHENTONG_SMB_HOST before running this script.
  exit /b 2
)
dir \\%SHENTONG_SMB_HOST%\C$
echo Exit code: %errorlevel%
