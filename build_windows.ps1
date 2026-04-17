# build_windows.ps1
# Build DM8 Export Tool Windows NSIS installer
# Requires Visual Studio 2022 with C++ workload
# Output: src-tauri/target/release/bundle/nsis/DM8 Export Tool_0.1.0_x64-setup.exe

$ErrorActionPreference = "Stop"
$ProjectRoot = Split-Path -Parent $MyInvocation.MyCommand.Path

# ── 1. Find vcvarsall.bat ─────────────────────────────────────────────────
$vcvars = $null
$candidates = @(
    "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvarsall.bat",
    "C:\Program Files\Microsoft Visual Studio\2022\Professional\VC\Auxiliary\Build\vcvarsall.bat",
    "C:\Program Files\Microsoft Visual Studio\2022\Enterprise\VC\Auxiliary\Build\vcvarsall.bat",
    "C:\Program Files\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvarsall.bat",
    "C:\Program Files (x86)\Microsoft Visual Studio\2019\Community\VC\Auxiliary\Build\vcvarsall.bat",
    "C:\Program Files (x86)\Microsoft Visual Studio\2019\BuildTools\VC\Auxiliary\Build\vcvarsall.bat"
)
foreach ($c in $candidates) {
    if (Test-Path $c) { $vcvars = $c; break }
}
if (-not $vcvars) {
    Write-Error "vcvarsall.bat not found. Install VS 2022 Build Tools from: https://aka.ms/vs/17/release/vs_BuildTools.exe"
    exit 1
}
Write-Host "Using VS: $vcvars"

# ── 2. Capture VS env vars via temp batch file ────────────────────────────
# Using Start-Process so that multi-char env values (LIB, PATH) survive correctly.
$tempFile = [System.IO.Path]::GetTempFileName() + ".txt"
$tempBat  = [System.IO.Path]::GetTempFileName() + ".bat"
Set-Content -Path $tempBat -Value "@echo off`r`ncall `"$vcvars`" x64 >nul 2>&1`r`nset > `"$tempFile`"`r`n" -Encoding ASCII
Start-Process -FilePath "cmd.exe" -ArgumentList "/c", $tempBat -Wait -NoNewWindow
Remove-Item $tempBat -Force
if (Test-Path $tempFile) {
    Get-Content $tempFile -Encoding Default | ForEach-Object {
        $idx = $_.IndexOf('=')
        if ($idx -gt 0) {
            $n = $_.Substring(0, $idx).Trim()
            $v = $_.Substring($idx + 1)
            if ($n -and $n.Length -lt 200) {
                [System.Environment]::SetEnvironmentVariable($n, $v, "Process")
            }
        }
    }
    Remove-Item $tempFile -Force
}
Write-Host "MSVC environment applied"

# ── 3. Set Windows SDK lib/include paths ─────────────────────────────────
# If using a custom NuGet-based SDK at ~/.winsdk/ (instead of a full Windows SDK install):
$winsdkUm   = "$env:USERPROFILE\.winsdk\Microsoft.Windows.SDK.CPP.x64\c\um\x64"
$winsdkUcrt = "$env:USERPROFILE\.winsdk\Microsoft.Windows.SDK.CPP.x64\c\ucrt\x64"
$msvcVersion = (Get-ChildItem "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Tools\MSVC" -ErrorAction SilentlyContinue |
    Sort-Object Name | Select-Object -Last 1).Name
$msvcLib = "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Tools\MSVC\$msvcVersion\lib\x64"
$msvcInc = "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Tools\MSVC\$msvcVersion\include"
$sdkBase = "$env:USERPROFILE\.winsdk\Microsoft.Windows.SDK.CPP\c\Include\10.0.22621.0"

if (Test-Path $winsdkUm) {
    $env:LIB     = "$winsdkUm;$winsdkUcrt;$msvcLib;$env:LIB"
    $env:INCLUDE = "$msvcInc;$sdkBase\ucrt;$sdkBase\um;$sdkBase\shared;$env:INCLUDE"
    Write-Host "Custom SDK paths set from $env:USERPROFILE\.winsdk\"
} else {
    $env:LIB = "$msvcLib;$env:LIB"
    Write-Host "Standard SDK LIB set"
}

# ── 4. Add rc.exe and DM8 Windows driver dir to PATH ─────────────────────
# rc.exe from NuGet Microsoft.Windows.SDK.BuildTools, placed in ~/.winsdk/buildtools/
$rcDir = "$env:USERPROFILE\.winsdk\buildtools"
if (Test-Path $rcDir) {
    $env:PATH = "$rcDir;$env:PATH"
    Write-Host "rc.exe dir added: $rcDir"
} else {
    Write-Warning "rc.exe dir not found at $rcDir — resource compilation may fail"
}
$driverDirs = @(
    "drivers\dm8\windows",
    "drivers\kingbase\X64_Windows\odbc\x64_ANSI_Release",
    "drivers\postgresql\windows",
    "drivers\shentong\windows"
)
foreach ($relativeDriverDir in $driverDirs) {
    $driverDir = Join-Path $ProjectRoot $relativeDriverDir
    if (Test-Path $driverDir) {
        $env:PATH = "$driverDir;$env:PATH"
        Write-Host "Driver dir added: $driverDir"
    }
}

# ── 5. Remove ReadOnly attribute from driver DLLs (source + any cached copies) ──
# DLL files copied from DM8 install have ReadOnly set, causing os error 5 in build.
Write-Host "Clearing ReadOnly from driver DLLs..."
$driverRoots = @(
    "drivers\dm8",
    "drivers\kingbase\X64_Windows",
    "drivers\postgresql\windows",
    "drivers\shentong\windows"
)
foreach ($relativeDriverRoot in $driverRoots) {
    $driverRoot = Join-Path $ProjectRoot $relativeDriverRoot
    Get-ChildItem $driverRoot -Recurse -ErrorAction SilentlyContinue | ForEach-Object {
        if ($_.Attributes -band [System.IO.FileAttributes]::ReadOnly) {
            $_.Attributes = $_.Attributes -band (-bnot [System.IO.FileAttributes]::ReadOnly)
        }
    }
}
$targetDrivers = Join-Path $ProjectRoot "src-tauri\target\release\drivers"
if (Test-Path $targetDrivers) {
    Remove-Item $targetDrivers -Recurse -Force
    Write-Host "Removed stale cached driver copies from target dir"
}

# ── 6. Install frontend dependencies ─────────────────────────────────────
Write-Host "Installing frontend dependencies..."
Push-Location (Join-Path $ProjectRoot "frontend")
& npm.cmd install
if ($LASTEXITCODE -ne 0) { Write-Error "npm install failed"; exit 1 }
Pop-Location

# ── 7. Build frontend ────────────────────────────────────────────────────
Write-Host "Building frontend..."
Push-Location (Join-Path $ProjectRoot "frontend")
& npm.cmd run build
if ($LASTEXITCODE -ne 0) { Write-Error "Frontend build failed"; exit 1 }
Pop-Location

# ── 8. Run Tauri NSIS build (from project root where src-tauri/ lives) ───
Write-Host "Starting Tauri NSIS build (may take 5-15 minutes on first run)..."
Push-Location $ProjectRoot
& node "frontend\node_modules\@tauri-apps\cli\tauri.js" build --bundles nsis
$buildExit = $LASTEXITCODE
Pop-Location

if ($buildExit -ne 0) { Write-Error "Tauri build failed ($buildExit)"; exit $buildExit }

# ── 9. Report output ─────────────────────────────────────────────────────
$bundleDir = Join-Path $ProjectRoot "src-tauri\target\release\bundle\nsis"
Write-Host "`nBuild successful! Installer:"
Get-ChildItem $bundleDir -Filter "*.exe" -ErrorAction SilentlyContinue |
    ForEach-Object { Write-Host "  $($_.FullName)" }
