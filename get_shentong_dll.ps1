$target = "\\192.168.3.34\C$"
$user = "192.168.3.34\Administrator"
$pass = ConvertTo-SecureString "123456" -AsPlainText -Force
$cred = New-Object System.Management.Automation.PSCredential($user, $pass)

# Try mapping the share
$drive = New-PSDrive -Name S -PSProvider FileSystem -Root $target -Credential $cred -ErrorAction Stop
Write-Host "Mapped share successfully"

# Look for ShenTong installation
$candidates = @("S:\SZ_OSCAR", "S:\ShenTong", "S:\OSCAR", "S:\Program Files\ShenTong", "S:\Program Files (x86)\ShenTong")
foreach ($dir in $candidates) {
    if (Test-Path $dir) {
        Write-Host "Found ShenTong at: $dir"
        # Find aci.dll
        Get-ChildItem -Path $dir -Filter "aci.dll" -Recurse -ErrorAction SilentlyContinue | ForEach-Object {
            Write-Host "Found aci.dll: $($_.FullName)"
        }
        # List top-level structure
        Get-ChildItem -Path $dir -ErrorAction SilentlyContinue | Select-Object Name
    }
}

# Also search the whole C drive for ShenTong
Write-Host "Searching for SZ_OSCAR_HOME..."
Get-ChildItem -Path "S:\" -Filter "SZ_OSCAR*" -ErrorAction SilentlyContinue | Select-Object FullName

Remove-PSDrive -Name S -ErrorAction SilentlyContinue
