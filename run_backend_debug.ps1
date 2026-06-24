$repoRoot = $PSScriptRoot

# Setup Visual Studio environment.
$vcvars = "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvarsall.bat"
if (Test-Path $vcvars) {
    $tempFile = [System.IO.Path]::GetTempFileName() + ".txt"
    $tempBat = [System.IO.Path]::GetTempFileName() + ".bat"

    $batContent = "@echo off`r`ncall `"$vcvars`" x64 > nul 2>&1`r`nset > `"$tempFile`"`r`n"
    Set-Content -Path $tempBat -Value $batContent -Encoding ASCII
    Start-Process -FilePath "cmd.exe" -ArgumentList "/c", $tempBat -Wait -NoNewWindow

    if (Test-Path $tempFile) {
        Get-Content $tempFile -Encoding Default | ForEach-Object {
            $idx = $_.IndexOf('=')
            if ($idx -gt 0) {
                $name = $_.Substring(0, $idx).Trim()
                $value = $_.Substring($idx + 1)
                if ($name -ne "" -and $name.Length -lt 200) {
                    [System.Environment]::SetEnvironmentVariable($name, $value, "Process")
                }
            }
        }
        Remove-Item $tempFile -Force
    }
    Remove-Item $tempBat -Force
}

# Optional downloaded SDK/MSVC paths used by this workstation.
$winsdkUm = "C:\Users\hifar\.winsdk\Microsoft.Windows.SDK.CPP.x64\c\um\x64"
$winsdkUcrt = "C:\Users\hifar\.winsdk\Microsoft.Windows.SDK.CPP.x64\c\ucrt\x64"
$msvcLib = "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Tools\MSVC\14.44.35207\lib\x64"
$env:LIB = "$winsdkUm;$winsdkUcrt;$msvcLib;$env:LIB"

$sdkBase = "C:\Users\hifar\.winsdk\Microsoft.Windows.SDK.CPP\c\Include\10.0.22621.0"
$msvcInc = "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Tools\MSVC\14.44.35207\include"
$env:INCLUDE = "$msvcInc;$sdkBase\ucrt;$sdkBase\um;$sdkBase\shared;$env:INCLUDE"

$driverDir = Join-Path $repoRoot "drivers\dm8\windows"
$env:DM8_DRIVER_PATH = Join-Path $driverDir "dodbc.dll"
$env:PATH = "$driverDir;$env:PATH"
$env:RUST_LOG = "dm8_export_backend=debug,tower_http=debug"

# Do not set DM8_ODBC_DRIVER to a DLL path. Windows ODBC DM resolves by HKLM driver name.
Remove-Item Env:\DM8_ODBC_DRIVER -ErrorAction SilentlyContinue

Write-Host "DM8_DRIVER_PATH = $env:DM8_DRIVER_PATH"
Write-Host "Expected connection-string driver name: Amarone DM8 ODBC Driver"

$driverName = "Amarone DM8 ODBC Driver"
$hklmDriversPath = "HKLM:\SOFTWARE\ODBC\ODBCINST.INI\ODBC Drivers"
$alreadyRegistered = $false
try {
    Get-ItemProperty -Path $hklmDriversPath -Name $driverName -ErrorAction Stop | Out-Null
    $alreadyRegistered = $true
    Write-Host "DM8 ODBC driver already registered in HKLM."
} catch {}

if (-not $alreadyRegistered) {
    Write-Host "DM8 ODBC driver not in HKLM. Launching elevated registration (UAC prompt may appear)..."
    $registerScript = Join-Path $repoRoot "scripts\register_dm8_odbc.ps1"
    $proc = Start-Process powershell.exe `
        -ArgumentList "-ExecutionPolicy Bypass -File `"$registerScript`" -DriverDll `"$env:DM8_DRIVER_PATH`" -DriverName `"$driverName`"" `
        -Verb RunAs -Wait -PassThru
    if ($proc.ExitCode -ne 0) {
        Write-Host "WARNING: ODBC driver registration failed or was cancelled (ExitCode=$($proc.ExitCode))."
    }
}

Set-Location (Join-Path $repoRoot "backend")
& cargo run
