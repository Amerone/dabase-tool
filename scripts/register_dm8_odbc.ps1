# register_dm8_odbc.ps1
# 以管理员身份运行，将 DM8 ODBC 驱动注册到 HKLM
param(
    [Parameter(Mandatory=$true)]
    [string]$DriverDll,

    [string]$DriverName = "Amarone DM8 ODBC Driver"
)

$driverName = $DriverName

try {
    # HKLM\SOFTWARE\ODBC\ODBCINST.INI\ODBC Drivers
    $driversKey = "HKLM:\SOFTWARE\ODBC\ODBCINST.INI\ODBC Drivers"
    if (-not (Test-Path $driversKey)) { New-Item -Path $driversKey -Force | Out-Null }
    New-ItemProperty -Path $driversKey -Name $driverName -Value "Installed" -PropertyType String -Force | Out-Null

    # HKLM\SOFTWARE\ODBC\ODBCINST.INI\DM8 ODBC Driver
    $driverKey = "HKLM:\SOFTWARE\ODBC\ODBCINST.INI\$driverName"
    if (-not (Test-Path $driverKey)) { New-Item -Path $driverKey -Force | Out-Null }
    New-ItemProperty -Path $driverKey -Name "Driver"        -Value $DriverDll -PropertyType String -Force | Out-Null
    New-ItemProperty -Path $driverKey -Name "Setup"         -Value $DriverDll -PropertyType String -Force | Out-Null
    New-ItemProperty -Path $driverKey -Name "APILevel"      -Value "1"        -PropertyType String -Force | Out-Null
    New-ItemProperty -Path $driverKey -Name "DriverODBCVer" -Value "03.00"    -PropertyType String -Force | Out-Null
    New-ItemProperty -Path $driverKey -Name "FileUsage"     -Value "0"        -PropertyType String -Force | Out-Null

    Write-Host "SUCCESS: DM8 ODBC driver registered to HKLM: $DriverDll"
    exit 0
} catch {
    Write-Host "ERROR: $_"
    exit 1
}
