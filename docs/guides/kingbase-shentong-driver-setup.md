# Kingbase/Shentong Driver Setup Guide (Windows + Ubuntu)

## Scope

This guide is for phase-1 support in this project:

- KingbaseES: `connection + table browsing + table details` via ODBC
- Shentong/OSCAR: `connection + table browsing + table details` via ODBC
- Export is still DM8-only in phase-1

## Required Deliverables From Vendor

When requesting installation packages from Kingbase/Shentong vendors, ask for:

1. 64-bit ODBC driver package for Windows
2. 64-bit ODBC driver package for Linux (x86_64, Ubuntu 20.04/22.04)
3. Unicode ODBC driver variant (if separate)
4. `odbcinst.ini` / `odbc.ini` registration examples
5. Driver name and driver shared-library full path

## Windows Setup

1. Install vendor client/ODBC package (x64).
2. Open `ODBC Data Sources (64-bit)` and verify driver appears.
3. Confirm driver names (example values):
   - Kingbase: `KingbaseES ODBC Driver`
   - Shentong: `OSCAR ODBC DRIVER`
4. Keep DSN-less mode in this project. No DSN is required.

If you place bundled DLLs under project `drivers/*/windows`, backend startup
will try to register them automatically in Windows registry.

### Windows Validation

Run PowerShell:

```powershell
Get-OdbcDriver | Where-Object { $_.Name -match 'Kingbase|OSCAR|Shentong' } |
  Select-Object Name,Platform,Attribute
```

Expected: target driver is listed as `64-bit`.

## Ubuntu Setup

1. Install unixODBC:

```bash
sudo apt-get update
sudo apt-get install -y unixodbc unixodbc-dev
```

2. Install vendor driver package (Kingbase/Shentong Linux x64).
3. Register driver in `/etc/odbcinst.ini`.

Example (`KingbaseES`):

```ini
[KingbaseES ODBC Driver]
Description=KingbaseES ODBC Driver
Driver=/opt/Kingbase/ES/odbc/kdbodbcw.so
```

Example (`Shentong` placeholder, replace with vendor real path):

```ini
[OSCAR ODBC DRIVER]
Description=Shentong OSCAR ODBC Driver
Driver=/opt/shentong/odbc/liboscarodbcw.so
```

4. Verify registration:

```bash
odbcinst -q -d
```

5. Optional path check:

```bash
ldd /opt/Kingbase/ES/odbc/kdbodbcw.so
```

## Project Runtime Variables

This project uses DSN-less ODBC connection strings and driver-name lookup.

Optional env variables:

- `KINGBASE_ODBC_DRIVER` (default: `KingbaseES ODBC Driver`)
- `SHENTONG_ODBC_DRIVER` (default: `OSCAR ODBC DRIVER`)
- `KINGBASE_ODBC_DRIVER_PATH` (explicit driver file path)
- `SHENTONG_ODBC_DRIVER_PATH` (explicit driver file path)

If unset, backend uses the defaults above.

## Bundled Driver Directory Convention

Per project convention, drivers are stored in repo `drivers/` directory.

Recommended layout:

```text
drivers/
  kingbase/
    windows/
      kdbodbcw.dll
    libkdbodbcw.so
  shentong/
    windows/
      oscarodbcw.dll
    liboscarodbcw.so
```

Backend probing behavior:

1. First, use `*_ODBC_DRIVER_PATH` if set and file exists
2. Then probe bundled paths under `drivers/kingbase` or `drivers/shentong`
3. Finally fall back to ODBC driver name lookup (`*_ODBC_DRIVER`)

## Notes About MySQL

MySQL in this project does **not** use ODBC.  
It uses Rust native driver (`sqlx/mysql`).

## Troubleshooting

1. Error: driver not found
   - Check `odbcinst -q -d` (Ubuntu) or ODBC Data Sources (Windows).
   - Confirm env variable matches exact registered driver name.
2. Error: cannot load shared library
   - Run `ldd <driver.so>` and install missing dependencies.
3. Error: auth or schema denied
   - Verify account can access target schema/database.
4. Error: metadata query fails on non-standard catalog
   - Vendor may customize `information_schema`; provide sample DB for compatibility patching.
