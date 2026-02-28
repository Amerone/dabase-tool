# Setup Visual Studio environment and run backend
$vcvars = "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvarsall.bat"
$tempFile = [System.IO.Path]::GetTempFileName() + ".txt"
$tempBat = [System.IO.Path]::GetTempFileName() + ".bat"

# Write a temp batch file that sets VS env and outputs all env vars
$batContent = "@echo off`r`ncall `"$vcvars`" x64 > nul 2>&1`r`nset > `"$tempFile`"`r`n"
Set-Content -Path $tempBat -Value $batContent -Encoding ASCII

$proc = Start-Process -FilePath "cmd.exe" -ArgumentList "/c", $tempBat -Wait -NoNewWindow -PassThru

# Read and apply env vars
if (Test-Path $tempFile) {
    $envLines = Get-Content $tempFile -Encoding Default
    foreach ($line in $envLines) {
        $idx = $line.IndexOf('=')
        if ($idx -gt 0) {
            $name = $line.Substring(0, $idx).Trim()
            $value = $line.Substring($idx + 1)
            if ($name -ne "" -and $name.Length -lt 200) {
                [System.Environment]::SetEnvironmentVariable($name, $value, "Process")
            }
        }
    }
    Remove-Item $tempFile -Force
}
Remove-Item $tempBat -Force

# Add downloaded Windows SDK libs to LIB path
$winsdkUm   = "C:\Users\hifar\.winsdk\Microsoft.Windows.SDK.CPP.x64\c\um\x64"
$winsdkUcrt = "C:\Users\hifar\.winsdk\Microsoft.Windows.SDK.CPP.x64\c\ucrt\x64"
$msvcLib    = "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Tools\MSVC\14.44.35207\lib\x64"

$env:LIB = "$winsdkUm;$winsdkUcrt;$msvcLib;$env:LIB"
Write-Host "LIB = $env:LIB"

# Add Windows SDK include paths
$sdkBase    = "C:\Users\hifar\.winsdk\Microsoft.Windows.SDK.CPP\c\Include\10.0.22621.0"
$msvcInc    = "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Tools\MSVC\14.44.35207\include"
$env:INCLUDE = "$msvcInc;$sdkBase\ucrt;$sdkBase\um;$sdkBase\shared;$env:INCLUDE"
Write-Host "INCLUDE = $env:INCLUDE"

# Set DM8 driver path
$env:DM8_DRIVER_PATH = "E:\self\tool-database\drivers\dm8\windows\dodbc.dll"
$env:PATH = "E:\self\tool-database\drivers\dm8\windows;$env:PATH"

# Ensure DM8 ODBC driver is registered in HKLM (Windows ODBC DM only reads HKLM)
$driverName = "DM8 ODBC Driver"
$hklmDriversPath = "HKLM:\SOFTWARE\ODBC\ODBCINST.INI\ODBC Drivers"
$alreadyRegistered = $false
try {
    Get-ItemProperty -Path $hklmDriversPath -Name $driverName -ErrorAction Stop | Out-Null
    $alreadyRegistered = $true
    Write-Host "DM8 ODBC driver already registered in HKLM."
} catch {}

if (-not $alreadyRegistered) {
    Write-Host "Registering DM8 ODBC driver (UAC prompt may appear)..."
    $registerScript = "$PSScriptRoot\scripts\register_dm8_odbc.ps1"
    $proc = Start-Process powershell.exe `
        -ArgumentList "-ExecutionPolicy Bypass -File `"$registerScript`" -DriverDll `"$env:DM8_DRIVER_PATH`"" `
        -Verb RunAs -Wait -PassThru
    if ($proc.ExitCode -ne 0) {
        Write-Host "WARNING: ODBC driver registration failed. Run scripts\register_dm8_odbc.ps1 as Administrator."
    }
}

# Run backend
Set-Location "E:\self\tool-database\backend"
& cargo run
